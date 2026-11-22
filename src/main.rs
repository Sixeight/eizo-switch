use std::{
    process::{Child, Command},
    thread::sleep,
    time::{Duration, Instant},
};

use anyhow::{bail, Context, Result};
use clap::Parser;
use hidapi::{HidApi, HidDevice};

const VENDOR_ID: u16 = 0x056d;
const EV3895_PRODUCT_ID: u16 = 0x4065;
const REPORT_LEN: usize = 40;
const BEFORE_TIMEOUT: Duration = Duration::from_secs(10);
const ID_INPUT_SOURCE: u16 = 0x48;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Input {
    DisplayPort,
    UsbC,
    Hdmi1,
    Hdmi2,
}

impl Input {
    const ALL: [Input; 4] = [Input::DisplayPort, Input::UsbC, Input::Hdmi1, Input::Hdmi2];

    fn code(self) -> u16 {
        match self {
            Input::DisplayPort => 0x0300,
            Input::UsbC => 0x0301,
            Input::Hdmi1 => 0x0400,
            Input::Hdmi2 => 0x0401,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Input::DisplayPort => "dp",
            Input::UsbC => "usb-c",
            Input::Hdmi1 => "hdmi1",
            Input::Hdmi2 => "hdmi2",
        }
    }

    fn from_code(code: u16) -> Option<Input> {
        Input::ALL.into_iter().find(|i| i.code() == code)
    }

    fn parse(s: &str) -> Option<Input> {
        match s.to_ascii_lowercase().as_str() {
            "dp" | "displayport" => Some(Input::DisplayPort),
            "usb-c" | "usbc" | "typec" | "type-c" => Some(Input::UsbC),
            "hdmi1" | "hdmi" => Some(Input::Hdmi1),
            "hdmi2" => Some(Input::Hdmi2),
            _ => None,
        }
    }
}

/// Feature report that asks the monitor for the value of `id`.
fn get_request(id: u16) -> [u8; REPORT_LEN] {
    let mut buf = [0u8; REPORT_LEN];
    buf[..5].copy_from_slice(&[0x03, 0x01, 0xff, id as u8, (id >> 8) as u8]);
    buf
}

/// Feature report that sets the value of `id`.
fn set_request(id: u16, value: u16) -> [u8; REPORT_LEN] {
    let mut buf = [0u8; REPORT_LEN];
    buf[..9].copy_from_slice(&[
        0x02,
        0x01,
        0xff,
        id as u8,
        (id >> 8) as u8,
        0,
        0,
        value as u8,
        (value >> 8) as u8,
    ]);
    buf
}

fn check_ack(ack: &[u8], sent: &[u8]) -> Result<()> {
    if ack.len() < 7 || ack[0] != 0x07 || ack[1..7] != sent[1..7] {
        bail!("monitor did not acknowledge the request: {ack:02x?}");
    }
    Ok(())
}

fn read_ack(dev: &HidDevice, sent: &[u8]) -> Result<()> {
    let mut ack = [0u8; 8];
    ack[0] = 0x07;
    let n = dev
        .get_feature_report(&mut ack)
        .context("read ack report")?;
    check_ack(&ack[..n], sent)
}

fn get_value(dev: &HidDevice, id: u16) -> Result<u16> {
    let req = get_request(id);
    dev.send_feature_report(&req).context("send get request")?;
    let mut res = [0u8; REPORT_LEN];
    res[0] = 0x03;
    dev.get_feature_report(&mut res)
        .context("read value report")?;
    if res[..7] != req[..7] {
        bail!("unexpected reply: {:02x?}", &res[..9]);
    }
    read_ack(dev, &req)?;
    Ok(u16::from(res[8]) << 8 | u16::from(res[7]))
}

fn set_value(dev: &HidDevice, id: u16, value: u16) -> Result<()> {
    let req = set_request(id, value);
    dev.send_feature_report(&req).context("send set request")?;
    sleep(Duration::from_millis(50));
    let mut ack = [0u8; 8];
    ack[0] = 0x07;
    match dev.get_feature_report(&mut ack) {
        Ok(n) => check_ack(&ack[..n], &req),
        // Switching the input also moves the monitor's USB hub to the other
        // PC, so the device can vanish before the acknowledgement is read.
        Err(_) => Ok(()),
    }
}

fn open() -> Result<HidDevice> {
    let api = HidApi::new().context("init hidapi")?;
    api.open(VENDOR_ID, EV3895_PRODUCT_ID)
        .context("open EIZO EV3895 (is the monitor's USB connected to this machine?)")
}

#[derive(Parser)]
#[command(version, about = "Show or switch the EIZO EV3895 input over USB HID")]
struct Cli {
    /// Input to switch to: dp, usb-c, hdmi1, hdmi2. Omit to print the current input.
    input: Option<String>,

    /// Shell command to run before switching, for example to wake the target PC's display.
    /// A failure is reported and the switch continues.
    #[arg(long, value_name = "COMMAND", requires = "input")]
    before: Option<String>,
}

/// Starts `command` in the background. Returns `None` when it cannot be started.
fn spawn_before(command: &str) -> Option<Child> {
    Command::new("sh").args(["-c", command]).spawn().ok()
}

/// Waits for `child` until `deadline` and kills it on overrun.
/// Returns whether it exited successfully in time.
fn wait_before(child: &mut Child, deadline: Instant) -> bool {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if Instant::now() < deadline => sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    // The command runs in the background while the switch happens, so a slow
    // command such as ssh does not delay the switch.
    let before = cli.before.as_deref().map(|command| {
        let deadline = Instant::now() + BEFORE_TIMEOUT;
        (command, spawn_before(command), deadline)
    });
    let result = run(&cli);
    if let Some((command, child, deadline)) = before {
        let succeeded = child.is_some_and(|mut child| wait_before(&mut child, deadline));
        if !succeeded {
            eprintln!("warning: --before command failed or timed out: {command}");
        }
    }
    result
}

fn run(cli: &Cli) -> Result<()> {
    let dev = open()?;
    match &cli.input {
        None => {
            let code = get_value(&dev, ID_INPUT_SOURCE)?;
            match Input::from_code(code) {
                Some(input) => println!("{}", input.name()),
                None => println!("unknown({code:04x})"),
            }
        }
        Some(name) => {
            let input = Input::parse(name)
                .with_context(|| format!("unknown input {name:?} (dp, usb-c, hdmi1, hdmi2)"))?;
            set_value(&dev, ID_INPUT_SOURCE, input.code())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_before(command: &str, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        spawn_before(command).is_some_and(|mut child| wait_before(&mut child, deadline))
    }

    #[test]
    fn test_set_request_matches_known_protocol_bytes() {
        let buf = set_request(0x48, 0x0301);
        assert_eq!(buf[..9], [0x02, 0x01, 0xff, 0x48, 0x00, 0, 0, 0x01, 0x03]);
        assert_eq!(buf.len(), 40);
    }

    #[test]
    fn test_get_request_matches_known_protocol_bytes() {
        let buf = get_request(0x48);
        assert_eq!(buf[..5], [0x03, 0x01, 0xff, 0x48, 0x00]);
    }

    #[test]
    fn test_check_ack_rejects_reply_for_another_request() {
        let sent = set_request(0x48, 0x0301);
        let mut ack = [0u8; 8];
        ack[..7].copy_from_slice(&sent[..7]);
        ack[0] = 0x07;
        assert!(check_ack(&ack, &sent).is_ok());
        ack[3] = 0x49;
        assert!(check_ack(&ack, &sent).is_err());
    }

    #[test]
    fn test_run_before_reports_exit_status() {
        let long = Duration::from_secs(5);
        assert!(run_before("true", long));
        assert!(!run_before("false", long));
        assert!(!run_before("exit 3", long));
    }

    #[test]
    fn test_run_before_kills_a_command_that_overruns() {
        let started = Instant::now();
        assert!(!run_before("sleep 5", Duration::from_millis(200)));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn test_input_names_round_trip_through_codes() {
        for input in Input::ALL {
            assert_eq!(Input::from_code(input.code()), Some(input));
            assert_eq!(Input::parse(input.name()), Some(input));
        }
    }
}
