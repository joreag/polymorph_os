use uart_16550::SerialPort;
use spin::Mutex;
use lazy_static::lazy_static;
use x86_64::instructions::port::Port;

lazy_static! {
    pub static ref SERIAL1: Mutex<SerialPort> = {
        let mut serial_port = unsafe { SerialPort::new(0x3F8) };
        serial_port.init();
        
        // [MICT: HARDWARE OVERRIDE] Enable the "Received Data Available" Interrupt
        unsafe {
            // The IER (Interrupt Enable Register) is exactly 1 byte after the base port
            let mut ier = Port::new(0x3F9);
            ier.write(0x01_u8); // Set bit 0 to enable Rx interrupts
        }
        
        Mutex::new(serial_port)
    };
}


#[doc(hidden)]
pub fn _print(args: ::core::fmt::Arguments) {
    use core::fmt::Write;
    use x86_64::instructions::interrupts;
    interrupts::without_interrupts(|| {
    SERIAL1
        .lock()
        .write_fmt(args)
        .expect("Printing to serial failed");
    });
}

/// Prints to the host through the serial interface.
#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {
        $crate::serial::_print(format_args!($($arg)*));
    };
}

/// Prints to the host through the serial interface, appending a newline.
#[macro_export]
macro_rules! serial_println {
    () => ($crate::serial_print!("\n"));
    ($fmt:expr) => ($crate::serial_print!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => ($crate::serial_print!(
        concat!($fmt, "\n"), $($arg)*));
}

#[test_case]
fn test_println_simple() {
    serial_println!("test_println_simple output");
}

