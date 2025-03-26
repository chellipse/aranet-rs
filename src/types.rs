use std::{
    fmt::{Display, Error as FmtError, Formatter},
    result::Result as StdResult,
};

#[derive(Debug)]
/// Internally represented as 20 * $temp_in_c
pub struct Temp(u16);

#[allow(dead_code)]
impl Temp {
    pub fn new(value: u16) -> Self {
        Self(value)
    }

    /// gives 10 * Int C
    pub fn c(&self) -> u16 {
        self.0 / 2
    }
    /// gives 10 * Int F
    pub fn f(&self) -> u16 {
        (((self.0 * 9) / 5) / 2) + 320
    }
    pub fn c_float(&self) -> f64 {
        self.0 as f64 / 20.0
    }
    pub fn f_float(&self) -> f64 {
        (self.0 as f64 / 20.0) * (9.0 / 5.0) + 32.0
    }
}

impl Display for Temp {
    fn fmt(&self, f: &mut Formatter) -> StdResult<(), FmtError> {
        write!(f, "{}", self.0)?;
        Ok(())
    }
}

#[derive(Debug)]
pub struct CurrentReading {
    pub c02: u16,
    pub temp: Temp,
    pub preasure: u16,
    pub humidity: u8,
    pub bat: u8,
    pub status: u8,
}

impl CurrentReading {
    pub fn print_oneline(&self, fahrenheit: bool) {
        println!(
            "{}ppm {:.2}°{} {}% {}hPa",
            self.c02,
            if fahrenheit {
                self.temp.f_float()
            } else {
                self.temp.c_float()
            },
            if fahrenheit { "F" } else { "C" },
            self.humidity,
            self.preasure / 10
        );
    }
}

impl Display for CurrentReading {
    fn fmt(&self, f: &mut Formatter) -> StdResult<(), FmtError> {
        writeln!(f, "CO2:         {}", self.c02)?;
        writeln!(
            f,
            "Temperature: {:.2}°C / {:.2}°F",
            self.temp.c_float(),
            self.temp.f_float(),
        )?;
        writeln!(f, "Humidity:    {}", self.humidity)?;
        writeln!(f, "Presure:     {}", self.preasure)?;
        writeln!(f, "Battery:     {}", self.bat)?;
        write!(f, "Status:      {}", self.status)?;
        Ok(())
    }
}
