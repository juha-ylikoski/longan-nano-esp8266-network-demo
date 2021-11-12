use core::{cell::RefCell, fmt, fmt::Write, str::from_utf8};

use longan_nano::{self, hal::{delay::McycleDelay, eclic::EclicExt, pac::{ECLIC, Interrupt, USART1, USART2, adc1::stat}, prelude::{_embedded_hal_blocking_delay_DelayMs, _embedded_hal_serial_Read}, serial::{Rx, Tx}, serial}};
use nb;
use riscv::interrupt;

pub enum Error {
    Error,
}
pub struct Esp8266<'a> {
    // rx: Rx<USART1>,
    rx: heapless::spsc::Consumer<'a, u8, 256>,
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
    ECLIC::setup(
        Interrupt::USART1,
        longan_nano::hal::eclic::TriggerType::FallingEdge,
        longan_nano::hal::eclic::Level::L0,
        longan_nano::hal::eclic::Priority::P0,
    );
}

fn enable_interrupts() {
    unsafe {
        interrupt::enable();
    }
}



static mut USART1READER: interrupt::Mutex<RefCell<Option<Usart1Reader>>> = interrupt::Mutex::new(RefCell::new(None));
#[export_name = "USART1"]
fn uart_interrupt() {
    interrupt::free(|_| {
        let reader = unsafe {
            USART1READER.get_mut().get_mut().as_mut().unwrap()
        }; 
        let data = reader.rx.read().unwrap();
        reader.buffer.enqueue(data).unwrap();
        ECLIC::unpend(Interrupt::USART1);
        panic!("foo");
    });
}


const SSID: Option<&str> = option_env!("SSID");
const PASSWORD: Option<&str> = option_env!("PASSWORD");

const DEFAULT_SSID: &str = "1234";
const DEFAULT_PASSWORD: &str = "1234";

///
/// At commands from <https://docs.espressif.com/projects/esp-at/en/latest/AT_Command_Set/>
///
/// commands do not include the "AT" prefix
#[macro_use]
mod AT_commands {

    use at_commands::builder::CommandBuilder;

    use super::{DEFAULT_PASSWORD, DEFAULT_SSID, PASSWORD, SSID};

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
        CommandBuilder::create_set(buf, true)
            .named("+CWJAP")
            .with_string_parameter(SSID.or(Some(DEFAULT_SSID)).unwrap())
            .with_string_parameter(PASSWORD.or(Some(DEFAULT_PASSWORD)).unwrap())
            .finish()
    }
}

impl Esp8266<'_> {
    pub fn new(rx: Rx<USART1>, tx: Tx<USART1>, delay: RefCell<McycleDelay>, tx2: Tx<USART2>) -> Self {

        static mut queue: heapless::spsc::Queue<u8, 256> = heapless::spsc::Queue::<u8, 256>::new();
        let (mut producer, mut consumer) = unsafe {queue.split()};
        let mut esp = Esp8266 { rx: consumer, tx, delay, tx2 };

        let usart1_reader = Usart1Reader {rx, buffer: producer};
        unsafe {
            USART1READER.get_mut().replace(Some(usart1_reader));
        }

        setup_uart1_interrupts();
        enable_interrupts();
    

        // Set echo off
        esp.tx.write_str(AT_commands::AT_PREFIX).unwrap();
        esp.tx.write_str(AT_commands::ECHO_OFF).unwrap();
        esp.tx.write_str(AT_commands::AT_LINE_ENDING).unwrap();
        esp.delay.get_mut().delay_ms(5000);

        esp.tx2.write_str("Emptying rx buffer\r\n").unwrap();

        // Empty the rx buffer of any possible junk
        loop {
            match esp.rx.dequeue() {
                Some(_) => (),
                None => break,
            }
        }

        esp.tx2.write_str("Rx buffer empty\r\n").unwrap();
        // loop {
        //     match esp.rx.read() {
        //         Ok(_) => (),
        //         Err(e) => match e {
        //             nb::Error::Other(_) => (),
        //             nb::Error::WouldBlock => break,
        //         },
        //     }
        // }
        esp
    }
    fn send_cmd(&mut self, cmd: &str) -> fmt::Result {
        self.tx2.write_str("Sending: ").unwrap();
        self.tx2.write_str(AT_commands::AT_PREFIX).unwrap();
        self.tx2.write_str(cmd).unwrap();
        self.tx2.write_str("\r\n").unwrap();
        self.tx.write_str(AT_commands::AT_PREFIX).unwrap();
        self.tx.write_str(cmd)?;
        self.tx.write_str(AT_commands::AT_LINE_ENDING)

    }
    fn read_command_output(&mut self) -> heapless::String<512> {
        let mut buffer = heapless::String::<512>::new();
        // let mut tx2_buf = heapless::String::<512>::new();
        const OK_ENDING: &str = "OK\r\n";
        const ERROR_ENDING: &str = "ERROR\r\n";
        let ok_len = OK_ENDING.len();
        let err_len = ERROR_ENDING.len();
        let mut would_block_counter = 0u16;

        self.tx2.write_str("Reading response\r\n").unwrap();
        loop {
            match self.rx.dequeue() {
                Some(val) => {
                    self.tx2.write_char(val as char).unwrap();
                    buffer.push(val as char).unwrap();
                    if &buffer[buffer.len()-ok_len..] == OK_ENDING
                        || &buffer[buffer.len()-err_len..]
                            == ERROR_ENDING
                    {
                        break;
                    }
                }
                None => (),
                // (e) => {
                //     match e {
                //         nb::Error::WouldBlock => {
                //             tx2_buf.push_str("Would block\r\n").unwrap();
                //             would_block_counter += 1;
                //         },
                //         nb::Error::Other(e) => {
                //             match e {
                //                 serial::Error::Framing => tx2_buf.push_str("Framing\r\n").unwrap(),
                //                 serial::Error::Noise => tx2_buf.push_str("Noise\r\n").unwrap(),
                //                 serial::Error::Overrun => tx2_buf.push_str("Overrun\r\n").unwrap(),
                //                 serial::Error::Parity => tx2_buf.push_str("Parity\r\n").unwrap(),
                //                 _ => tx2_buf.push_str("Other\r\n").unwrap(),
                //             }
                //         }
                //     }
                // }
            }
        }
        // self.tx2.write_str("Error: ").unwrap();
        // self.tx2.write_str(&tx2_buf).unwrap();
        self.tx2.write_str("\r\n").unwrap();
        self.tx2.write_str("Read cmd complete\n\r").unwrap();
        self.tx2.write_str(&buffer).unwrap();
        buffer
    }

    fn communicate(&mut self, cmd: &str) -> Result<heapless::String<512>, fmt::Error> {
        self.send_cmd(cmd)?;
        Ok(self.read_command_output())
    }

    pub fn at(&mut self) -> fmt::Result {
        match self.communicate(AT_commands::AT) {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        }
    }
    pub fn reset(&mut self) -> fmt::Result {
        self.send_cmd(AT_commands::RESET)?;
        loop {
            match self.rx.dequeue() {
                Some(_) => (),
                None => break,
            }
        }
        Ok(())
    }
    pub fn connect_wifi(&mut self) -> Result<(), fmt::Error> {
        let mut buf = [0u8; 128];
        let cmd = AT_commands::set_wifi_ap(&mut buf).unwrap();
        let cmd_str = from_utf8(cmd).unwrap();
        let mut set_mode = heapless::String::<64>::new();
        set_mode.push_str(AT_commands::AT_PREFIX).unwrap();
        set_mode.push_str(AT_commands::SET_STA_MODE).unwrap();
        self.communicate(&set_mode)?;
        self.communicate(cmd_str)?;
        Ok(())
    }
}
