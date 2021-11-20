use core::{cell::RefCell, fmt, fmt::Write, str::from_utf8};

use longan_nano::{self, hal::{delay::McycleDelay, eclic::{EclicExt, Level, LevelPriorityBits}, pac::{ECLIC, Interrupt, USART1, USART2, adc1::stat}, prelude::{_embedded_hal_blocking_delay_DelayMs, _embedded_hal_blocking_delay_DelayUs, _embedded_hal_serial_Read}, serial::{Rx, Tx}, serial}};
use nb;
use riscv::interrupt;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Esp8266Error {
    Error(heapless::String::<512>),
    FmtError(fmt::Error)
}
pub struct Esp8266 {
    rx: Rx<USART1>,
    // rx: heapless::spsc::Consumer<'a, u8, 256>,
    tx: Tx<USART1>,
    delay: RefCell<McycleDelay>,
    tx2: Tx<USART2>,
}

struct Usart1Reader<'a> {
    rx: Rx<USART1>,
    buffer: heapless::spsc::Producer<'a, u8, 256>
}

fn setup_uart1_interrupts() {
    ECLIC::reset();
    // Set global interrupt threshold level to lowest.
    // => All interrupts are handled.
    ECLIC::set_threshold_level(Level::L0);

    // Three bits for level, 1 for priority.
    ECLIC::set_level_priority_bits(LevelPriorityBits::L3P1);

    ECLIC::setup(
        Interrupt::USART1,
        longan_nano::hal::eclic::TriggerType::FallingEdge,
        longan_nano::hal::eclic::Level::L0,
        longan_nano::hal::eclic::Priority::P0,
    );

    unsafe {
        ECLIC::unmask(Interrupt::USART1);
    }
}

fn enable_interrupts() {
    unsafe {
        interrupt::enable();
    }
}



static mut USART1READER: interrupt::Mutex<RefCell<Option<Usart1Reader>>> = interrupt::Mutex::new(RefCell::new(None));
#[export_name = "USART1"]
fn uart_interrupt() {
    panic!("foo");
    interrupt::free(|_| {
        let reader = unsafe {
            USART1READER.get_mut().get_mut().as_mut().unwrap()
        }; 
        let data = reader.rx.read().unwrap();
        reader.buffer.enqueue(data).unwrap();
        ECLIC::unpend(Interrupt::USART1);
        
    });
}


const SSID: Option<&str> = option_env!("SSID");
const PASSWORD: Option<&str> = option_env!("PASSWORD");

const DEFAULT_SSID: &str = "1234";
const DEFAULT_PASSWORD: &str = "1234";

const SITE_IP_ADDR: &str = "192.168.0.126";
const SITE_PORT: u16 = 8000;

///
/// At commands from <https://docs.espressif.com/projects/esp-at/en/latest/AT_Command_Set/>
///
/// commands do not include the "AT" prefix
#[macro_use]
mod AT_commands {

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

    pub fn cipsend(length: usize) -> heapless::String::<32> {
        let mut s = heapless::String::<32>::new();
        s.push_str("+CIPSEND=").unwrap();
        s.push_str(&heapless::String::<8>::from(length as u8)).unwrap();
        s
    }

    pub const SET_TRANSPARENT_TRANSMISSION: &str = "+CIPMODE=0";
}

impl Esp8266 {
    pub fn new(rx: Rx<USART1>, tx: Tx<USART1>, delay: RefCell<McycleDelay>, tx2: Tx<USART2>) -> Self {
        let mut esp = Esp8266 { rx, tx, delay, tx2 };

        // Set echo off
        esp.tx.write_str(AT_commands::AT_PREFIX).unwrap();
        esp.tx.write_str(AT_commands::ECHO_OFF).unwrap();
        esp.tx.write_str(AT_commands::AT_LINE_ENDING).unwrap();

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
            self.tx2.write_str(AT_commands::AT_PREFIX).unwrap();
        }
        self.tx2.write_str(cmd).unwrap();
        self.tx2.write_str("\r\n").unwrap();
        if prefix {
            self.tx.write_str(AT_commands::AT_PREFIX)?;
        }
        self.tx.write_str(cmd)?;
        self.tx.write_str(AT_commands::AT_LINE_ENDING)

    }

    fn communicate(&mut self, cmd: &str, prefix: bool) -> Result<heapless::String<512>, Esp8266Error> {
        let mut buffer = heapless::String::<512>::new();
        const OK_ENDING: &str = "OK\r\n";
        const ERROR_ENDING: &str = "ERROR\r\n";
        const FAIL_ENDING: &str = "FAIL\r\n";
        let ok_len = OK_ENDING.len();
        let err_len = ERROR_ENDING.len();
        let fail_len = FAIL_ENDING.len();
        let mut would_block_counter = 0u16;

        // self.tx2.write_str("Reading response\r\n").unwrap();
        self.send_cmd(cmd, prefix).or_else(|e| Err(Esp8266Error::FmtError(e)))?;
        loop {
            match self.rx.read() {
                Ok(val) => {
                    buffer.push(val as char).unwrap();
                    if val as char == '\n' && buffer.len() > OK_ENDING.len() {
                        if &buffer[buffer.len()-ok_len..] == OK_ENDING
                                    || &buffer[buffer.len()-err_len..]
                                        == ERROR_ENDING || &buffer[buffer.len()-fail_len..]
                                        == FAIL_ENDING
                                {
                                    break;
                                }
                            }
                }
                Err(e) => {
                    match e {
                        nb::Error::WouldBlock => {
                            // would_block_counter += 1;
                            self.delay.get_mut().delay_us(1);
                            
                        },
                        nb::Error::Other(e) => {
                            // match e {
                            //     serial::Error::Framing => tx2_buf.push_str("Framing\r\n").unwrap(),
                            //     serial::Error::Noise => tx2_buf.push_str("Noise\r\n").unwrap(),
                            //     serial::Error::Overrun => tx2_buf.push_str("Overrun\r\n").unwrap(),
                            //     serial::Error::Parity => tx2_buf.push_str("Parity\r\n").unwrap(),
                            //     _ => tx2_buf.push_str("Other\r\n").unwrap(),
                            // };
                            would_block_counter += 1;
                        }
                    }
                }
            }
            // if would_block_counter > 65534 {
            //     break;
            // }
        }
        let mut stripped_buf =  buffer.trim_start_matches("\r\n");
        stripped_buf = stripped_buf.trim_end_matches("\r\n");
        let buffer_stripped = heapless::String::<512>::from(stripped_buf);
        self.tx2.write_str("Read cmd complete\n\r").unwrap();
        self.tx2.write_str(&buffer_stripped).unwrap();
        self.tx2.write_str("\r\n").unwrap();
        if buffer_stripped.contains("busy") {
        }
        if buffer_stripped.contains("OK") {
            Ok(buffer_stripped)
        } else {
            Err(Esp8266Error::Error(buffer_stripped))
        }
    }

    pub fn at(&mut self) -> Result<(), Esp8266Error> {
        match self.communicate(AT_commands::AT, true) {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        }
    }
    pub fn reset(&mut self) -> fmt::Result {
        self.send_cmd(AT_commands::RESET, true)?;
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
        let cmd = AT_commands::set_wifi_ap(&mut buf).unwrap();
        let cmd_str = from_utf8(cmd).unwrap();
        let mut set_mode = heapless::String::<64>::new();
        // set_mode.push_str(AT_commands::AT_PREFIX).unwrap();
        set_mode.push_str(AT_commands::SET_STA_MODE).unwrap();
        self.communicate(&set_mode, true)?;
        self.delay.get_mut().delay_ms(1000);
        self.communicate(cmd_str, true)?;
        Ok(())
    }

    pub fn start_tcp_connection(&mut self) -> Result<(), Esp8266Error> {
        let mut buf = [0u8; 128];
        let cmd = AT_commands::start_tcp_connection(&mut buf).unwrap();
        let cmd_str = from_utf8(cmd).unwrap();
        self.communicate(cmd_str, true)?;
        self.communicate(AT_commands::SET_TRANSPARENT_TRANSMISSION, true)?;
        let http_cmd = "GET / HTTP/1.1\r\nHost: 192.168.0.126:8000\r\nAccept: */*\r\n\r\n";
        self.communicate(&AT_commands::cipsend(http_cmd.len()), true)?;
        let mut buf = heapless::String::<16384>::new();
        const CLOSED_ENDING: &str = "CLOSED\r\n";
        self.communicate(&http_cmd, false)?;
        loop {
            match self.rx.read() {
                Ok(v) => {
                    buf.push(v as char).unwrap();
                    if v as char == '\n' && buf.len() > CLOSED_ENDING.len() {
                        if &buf[buf.len()-CLOSED_ENDING.len()..] == CLOSED_ENDING
                                {
                                    break;
                                }
                            }
                },
                Err(_) => ()
            }
        }
        self.tx2.write_str(&buf).unwrap();
        Ok(())
    }
}
