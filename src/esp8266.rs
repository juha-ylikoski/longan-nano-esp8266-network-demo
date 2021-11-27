use core::{cell::RefCell, fmt, fmt::Write, str::from_utf8};
use longan_nano::{
    self,
    hal::{
        delay::McycleDelay,
        pac::{USART1, USART2},
        prelude::{_embedded_hal_blocking_delay_DelayMs, _embedded_hal_serial_Read},
        serial::{Rx, Tx},
    },
};
use nb;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Esp8266Error {
    Error(heapless::String<512>),
    FmtError(fmt::Error),
    GetError(heapless::String<128>),
    JsonError,
}
pub struct Esp8266 {
    rx: Rx<USART1>,
    tx: Tx<USART1>,
    delay: RefCell<McycleDelay>,
    tx2: Tx<USART2>,
}

pub struct HttpJsonResp {
    pub code: u16,
    pub http_resp: heapless::String<8192>,
    pub json: i32,
}

const SSID: Option<&str> = option_env!("SSID");
const PASSWORD: Option<&str> = option_env!("PASSWORD");

const DEFAULT_SSID: &str = "1234";
const DEFAULT_PASSWORD: &str = "1234";

const SITE_IP_ADDR: &str = "192.168.0.147";
const SITE_PORT: u16 = 5000;

fn http_get_payload() -> heapless::String<128> {
    // "GET / HTTP/1.1\r\nHost: 192.168.0.147:5000\r\nAccept: application/json\r\n\r\n";
    let mut get = heapless::String::<128>::from("GET / HTTP/1.1\r\nHost: ");
    get.push_str(SITE_IP_ADDR).unwrap();
    get.push_str(":").unwrap();
    get.push_str(&heapless::String::<8>::from(SITE_PORT))
        .unwrap();
    get.push_str("\r\nAccept: application/json\r\n\r\n")
        .unwrap();
    get
}

///
/// At commands from <https://docs.espressif.com/projects/esp-at/en/latest/AT_Command_Set/>
///
/// commands do not include the "AT" prefix
#[macro_use]
#[allow(dead_code)]
mod at_commands {

    use at_commands::builder::CommandBuilder;

    use super::{DEFAULT_PASSWORD, DEFAULT_SSID, PASSWORD, SITE_IP_ADDR, SITE_PORT, SSID};

    pub const AT_LINE_ENDING: &str = "\r\n";
    pub const AT_PREFIX: &str = "AT";

    // Basic AT commands
    pub const AT: &str = "";
    pub const RESET: &str = "+RST";
    pub const ECHO_OFF: &str = "E0";
    pub const ECHO_ON: &str = "E1";
    pub const UART_BAUDRATE_SET: &str = "AT+UART_DEF=9600,8,1,0,0";

    // Wi-Fi AT commands
    pub const SET_STA_MODE: &str = "+CWMODE=1";
    pub const QUERY_AP: &str = "+CWJAP?";

    pub fn set_wifi_ap<'a>(buf: &'a mut [u8]) -> Result<&'a [u8], usize> {
        CommandBuilder::create_set(buf, false)
            .named("+CWJAP")
            .with_string_parameter(SSID.or(Some(DEFAULT_SSID)).unwrap())
            .with_string_parameter(PASSWORD.or(Some(DEFAULT_PASSWORD)).unwrap())
            .finish()
    }

    pub fn start_tcp_connection<'a>(buf: &'a mut [u8]) -> Result<&'a [u8], usize> {
        CommandBuilder::create_set(buf, false)
            .named("+CIPSTART")
            .with_string_parameter("TCP")
            .with_string_parameter(SITE_IP_ADDR)
            .with_int_parameter(SITE_PORT)
            .finish()
    }

    pub fn cipsend(length: usize) -> heapless::String<32> {
        let mut s = heapless::String::<32>::new();
        s.push_str("+CIPSEND=").unwrap();
        s.push_str(&heapless::String::<8>::from(length as u8))
            .unwrap();
        s
    }

    pub const SET_TRANSPARENT_TRANSMISSION: &str = "+CIPMODE=0";
}

impl Esp8266 {
    pub fn new(
        rx: Rx<USART1>,
        tx: Tx<USART1>,
        delay: RefCell<McycleDelay>,
        tx2: Tx<USART2>,
    ) -> Self {
        let mut esp = Esp8266 { rx, tx, delay, tx2 };

        // Set echo off
        esp.tx.write_str(at_commands::AT_PREFIX).unwrap();
        esp.tx.write_str(at_commands::ECHO_OFF).unwrap();
        esp.tx.write_str(at_commands::AT_LINE_ENDING).unwrap();

        esp.tx2.write_str("Emptying rx buffer\r\n").unwrap();

        loop {
            match esp.rx.read() {
                Ok(_) => (),
                Err(e) => match e {
                    nb::Error::Other(_) => (),
                    nb::Error::WouldBlock => break,
                },
            }
        }
        esp.tx2.write_str("Rx buffer empty\r\n").unwrap();
        esp
    }
    fn send_cmd(&mut self, cmd: &str, prefix: bool) -> fmt::Result {
        self.tx2.write_str("Sending: ").unwrap();
        if prefix {
            self.tx2.write_str(at_commands::AT_PREFIX).unwrap();
        }
        self.tx2.write_str(cmd).unwrap();
        self.tx2.write_str("\r\n").unwrap();
        if prefix {
            self.tx.write_str(at_commands::AT_PREFIX)?;
        }
        self.tx.write_str(cmd)?;
        self.tx.write_str(at_commands::AT_LINE_ENDING)
    }

    fn communicate_no_tx2_write(
        &mut self,
        cmd: &str,
        prefix: bool,
    ) -> Result<heapless::String<512>, Esp8266Error> {
        let mut buffer = heapless::String::<512>::new();
        const OK_ENDING: &str = "OK\r\n";
        const ERROR_ENDING: &str = "ERROR\r\n";
        const FAIL_ENDING: &str = "FAIL\r\n";
        self.send_cmd(cmd, prefix)
            .or_else(|e| Err(Esp8266Error::FmtError(e)))?;
        loop {
            match self.rx.read() {
                Ok(val) => {
                    // self.tx2.write_char(val as char);
                    buffer.push(val as char).unwrap();
                    if val as char == '\n' && buffer.len() > OK_ENDING.len() {
                        if buffer.ends_with(OK_ENDING)
                            || buffer.ends_with(ERROR_ENDING)
                            || buffer.ends_with(FAIL_ENDING)
                        {
                            break;
                        }
                    }
                }
                Err(_) => {}
            }
        }
        let mut stripped_buf = buffer.trim_start_matches("\r\n");
        stripped_buf = stripped_buf.trim_end_matches("\r\n");
        let buffer_stripped = heapless::String::<512>::from(stripped_buf);
        if buffer_stripped.contains("OK") {
            Ok(buffer_stripped)
        } else {
            Err(Esp8266Error::Error(buffer_stripped))
        }
    }

    fn communicate(
        &mut self,
        cmd: &str,
        prefix: bool,
    ) -> Result<heapless::String<512>, Esp8266Error> {
        match self.communicate_no_tx2_write(cmd, prefix) {
            Ok(val) => {
                self.tx2.write_str("Read cmd complete: ").unwrap();
                self.tx2.write_str(&val).unwrap();
                self.tx2.write_str("\r\n").unwrap();
                Ok(val)
            }
            Err(e) => match e {
                Esp8266Error::Error(e) => {
                    self.tx2
                        .write_str("Read cmd complete. Got error: ")
                        .unwrap();
                    self.tx2.write_str(&e).unwrap();
                    self.tx2.write_str("\r\n").unwrap();
                    Err(Esp8266Error::Error(e))
                }
                Esp8266Error::FmtError(e) => Err(Esp8266Error::FmtError(e)),
                // Should never get here
                _ => panic!("Bad return value"),
            },
        }
    }

    pub fn at(&mut self) -> Result<(), Esp8266Error> {
        let res = self.communicate(at_commands::AT, true);
        match res {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        }
    }
    #[allow(dead_code)]
    pub fn reset(&mut self) -> fmt::Result {
        self.send_cmd(at_commands::RESET, true)?;
        loop {
            match self.rx.read() {
                Ok(_) => (),
                Err(_) => break,
            }
        }
        Ok(())
    }
    pub fn connect_wifi(&mut self) -> Result<(), Esp8266Error> {
        let mut buf = [0u8; 128];
        let cmd = at_commands::set_wifi_ap(&mut buf).unwrap();
        let cmd_str = from_utf8(cmd).unwrap();
        let mut set_mode = heapless::String::<64>::new();
        // set_mode.push_str(at_commands::AT_PREFIX).unwrap();
        set_mode.push_str(at_commands::SET_STA_MODE).unwrap();
        self.communicate(&set_mode, true)?;
        self.delay.get_mut().delay_ms(1000);
        self.communicate(cmd_str, true)?;
        Ok(())
    }

    pub fn get(&mut self) -> Result<HttpJsonResp, Esp8266Error> {
        let mut buf = [0u8; 128];
        let cmd = at_commands::start_tcp_connection(&mut buf).unwrap();
        let cmd_str = from_utf8(cmd).unwrap();
        self.communicate(cmd_str, true)?;
        self.communicate(at_commands::SET_TRANSPARENT_TRANSMISSION, true)?;
        // http://172.18.12.48:5000/
        let http_cmd = http_get_payload();
        self.communicate(&at_commands::cipsend(http_cmd.len()), true)?;
        let mut buf = heapless::String::<8192>::new();
        const CLOSED_ENDING: &str = "CLOSED\r\n";

        // For some reason if I use communicate_no_tx2_write previous at() stops working buffer.ends_with(OK_ENDING) if statement. Don't know what happens there might be
        // panic but without debugger could not determine reason. The only thing which I think might cause this is some code optimizations which don't like the prefix being false in
        // this and true in everything else but hard to say what it really is without debugger.
        // This operation needs to be relatively fast so no tx2 write after OK received because +IDP are going to be written just after OK received.

        let mut buffer = heapless::String::<128>::new();
        const OK_ENDING: &str = "OK\r\n";
        const ERROR_ENDING: &str = "ERROR\r\n";
        const FAIL_ENDING: &str = "FAIL\r\n";
        self.send_cmd(&http_cmd, false)
            .or_else(|e| Err(Esp8266Error::FmtError(e)))?;
        loop {
            match self.rx.read() {
                Ok(val) => {
                    buffer.push(val as char).unwrap();
                    if val as char == '\n' && buffer.len() > OK_ENDING.len() {
                        if buffer.ends_with(OK_ENDING)
                            || buffer.ends_with(ERROR_ENDING)
                            || buffer.ends_with(FAIL_ENDING)
                        {
                            break;
                        }
                    }
                }
                Err(_) => {}
            }
        }
        if !buffer.ends_with("OK\r\n") {
            self.tx2.write_str("GETERROR: \r\n").unwrap();
            self.tx2.write_str(&buffer).unwrap();
            self.tx2.write_str("\r\n").unwrap();
            return Err(Esp8266Error::GetError(heapless::String::<128>::from(
                buffer,
            )));
        }
        loop {
            match self.rx.read() {
                Ok(v) => {
                    buf.push(v as char).unwrap();
                    if v as char == '\n' {
                        if buf.contains(CLOSED_ENDING) {
                            break;
                        }
                    }
                }
                Err(_) => (),
            }
        }
        let mut http_resp = heapless::String::<8192>::new();

        let http_resp_start_index = buf.find("HTTP/1.0 ").unwrap() + "HTTP/1.0 ".len();
        let http_resp_code = &buf[http_resp_start_index..http_resp_start_index + 3];
        let http_resp_code = http_resp_code.parse::<u16>().unwrap();

        // IPD packet starts with "\r\nIPD,\d+:"
        static IPD_SEPARATOR: &str = "\r\n+IPD,";

        let start_index = buf.find("Content-Type: application/json");
        match start_index {
            Some(index) => {
                let mut resp = &buf[index..(buf.len() - CLOSED_ENDING.len())];
                if resp.contains(IPD_SEPARATOR) {
                    loop {
                        match resp.find(IPD_SEPARATOR) {
                            Some(index) => {
                                let mut ipd_len = 5;
                                if index > 1 {
                                    let start = &resp[..index - 1];
                                    http_resp.push_str(start).unwrap();
                                }
                                loop {
                                    if &resp[index + ipd_len..index + ipd_len + 1] != ":" {
                                        ipd_len += 1;
                                    } else {
                                        ipd_len += 1;
                                        break;
                                    }
                                }
                                let ipd_end_index = index + ipd_len;
                                resp = &resp[ipd_end_index..];
                            }
                            None => break,
                        }
                    }
                } else {
                    http_resp.push_str(&resp).unwrap()
                }
            }
            None => (),
        }
        self.tx2.write_str("\r\n\r\n\r\n").unwrap();
        self.tx2.write_str("resp:\r\n").unwrap();
        self.tx2.write_str(&http_resp).unwrap();

        let json_start = http_resp.find("\r\n\r\n").unwrap() + 4;
        let json_content = &http_resp[json_start..];
        let json_content = json_content.trim_end_matches("\n").trim_end_matches("\r");
        let json = json_content
            .parse::<i32>()
            .or_else(|_| Err(Esp8266Error::JsonError))?;

        Ok(HttpJsonResp {
            code: http_resp_code,
            http_resp,
            json,
        })
    }
}
