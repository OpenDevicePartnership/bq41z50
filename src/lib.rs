#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use device_driver::{Block, FieldsetMetadata};
use embedded_batteries_async::smart_battery;
use embedded_hal_async::delay::DelayNs;

#[allow(clippy::all)]
#[allow(clippy::pedantic)]
mod device {
    use embedded_batteries_async::smart_battery::ErrorCode;

    include!("device.rs");
}

mod sbs;
#[cfg(test)]
mod tests;

pub use crate::device::*;

const BQ41Z50_ADDR: u8 = 0x0B;
const LARGEST_REG_BYTES: usize = 64;
const LARGEST_CMD_BYTES: usize = 32;
const LARGEST_BUF_BYTES: usize = 32;
const SBS_ADDR_REG_SIZE: usize = 1;
const SMBUS_BLOCK_COUNT_SIZE_BYTES: usize = 1;
const MAC_CMD_ADDR_SIZE_BYTES: u8 = 2;
// A MAC response is a block read of 0x44: the count byte, then the echoed subcommand, then the data
const MAC_CMD_RX_HEADER_SIZE_BYTES: usize = SMBUS_BLOCK_COUNT_SIZE_BYTES + MAC_CMD_ADDR_SIZE_BYTES as usize;
const MAC_CMD_SBS_ADDR: u8 = 0x44;

// Special case MAC commands
const SECURITY_KEYS_CMD: [u8; MAC_CMD_ADDR_SIZE_BYTES as usize] = 0x0035u16.to_le_bytes();
const SECURITY_KEYS_DATA_LEN_BYTES: u8 = 8;
const SECURITY_KEYS_LEN_BYTES: u8 = SECURITY_KEYS_DATA_LEN_BYTES + MAC_CMD_ADDR_SIZE_BYTES;
const AUTH_KEY_CMD: [u8; MAC_CMD_ADDR_SIZE_BYTES as usize] = 0x0037u16.to_le_bytes();
const AUTH_KEY_DATA_LEN_BYTES: u8 = 16;
const AUTH_KEY_LEN_BYTES: u8 = AUTH_KEY_DATA_LEN_BYTES + MAC_CMD_ADDR_SIZE_BYTES;
const MFG_INFO_CMD: u8 = 0x70;
const CHRG_VOLTAGE_OVERRIDE_CMD: [u8; MAC_CMD_ADDR_SIZE_BYTES as usize] = 0x00B0u16.to_le_bytes();
const CHRG_VOLTAGE_OVERRIDE_SIZE_BYTES: u8 = 10;
const MFG_INFO_C_CMD: [u8; MAC_CMD_ADDR_SIZE_BYTES as usize] = 0x007Bu16.to_le_bytes();

// Signature MACs. The gauge NACKs reads of these while it computes the signature, so the read has to
// wait after the command is sent.
const INSTRUCTION_FLASH_SIGNATURE_CMD: u16 = 0x0004;
const STATIC_DF_SIGNATURE_CMD: u16 = 0x0005;
const ALL_DF_SIGNATURE_CMD: u16 = 0x0009;
const INSTRUCTION_FLASH_SIGNATURE_DELAY_MS: u32 = 500;
const STATIC_DF_SIGNATURE_DELAY_MS: u32 = 1000;
const ALL_DF_SIGNATURE_DELAY_MS: u32 = 1000;

#[derive(Debug)]
#[non_exhaustive]
pub enum Bq41z50Error<E: embedded_hal_async::i2c::Error> {
    Bus(E),
    BatteryStatus(device::BatteryStatus),
    DataTooLarge,
}

impl<E: embedded_hal_async::i2c::Error> embedded_batteries_async::smart_battery::Error for Bq41z50Error<E> {
    fn kind(&self) -> embedded_batteries_async::smart_battery::ErrorKind {
        match self {
            Bq41z50Error::Bus(_) => embedded_batteries_async::smart_battery::ErrorKind::CommError,
            Bq41z50Error::BatteryStatus(status) => {
                embedded_batteries_async::smart_battery::ErrorKind::BatteryStatus(status.ec())
            }
            Bq41z50Error::DataTooLarge => embedded_batteries_async::smart_battery::ErrorKind::Other,
        }
    }
}

impl<E: embedded_hal_async::i2c::Error> From<E> for Bq41z50Error<E> {
    fn from(err: E) -> Self {
        Bq41z50Error::Bus(err)
    }
}

/// Charging Voltage Override config struct used in MAC command 0x00B0, not used in R1
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ChargingVoltageOverride {
    pub low_temp_chrg_mv: u16,
    pub std_low_temp_chrg_mv: u16,
    pub std_hi_temp_chrg_mv: u16,
    pub hi_temp_chrg_mv: u16,
    pub recommended_temp_chrg_mv: u16,
}

#[derive(Debug)]
pub struct Interface<BUS: embedded_hal_async::i2c::I2c> {
    pub bus: BUS,
}

impl<BUS: embedded_hal_async::i2c::I2c> Interface<BUS> {
    pub fn new(bus: BUS) -> Self {
        Self { bus }
    }

    async fn mac_read(&mut self, mac_command: u16, read: &mut [u8]) -> Result<(), Bq41z50Error<BUS::Error>> {
        if read.len() > LARGEST_CMD_BYTES {
            return Err(Bq41z50Error::DataTooLarge);
        }

        self.mac_send(mac_command).await?;
        self.read_block(MAC_CMD_SBS_ADDR, MAC_CMD_RX_HEADER_SIZE_BYTES, read)
            .await
    }

    /// Write a MAC command to `ManufacturerBlockAccess()` without reading the response.
    async fn mac_send(&mut self, mac_command: u16) -> Result<(), Bq41z50Error<BUS::Error>> {
        let mut write_buf = [0u8; 2 + MAC_CMD_ADDR_SIZE_BYTES as usize];
        write_buf[0] = MAC_CMD_SBS_ADDR;
        write_buf[1] = MAC_CMD_ADDR_SIZE_BYTES;
        write_buf[2..4].copy_from_slice(&mac_command.to_le_bytes());

        self.bus
            .write(BQ41Z50_ADDR, &write_buf)
            .await
            .map_err(Bq41z50Error::Bus)
    }

    /// `SMBus` block read of `command`, dropping the `header_len` bytes the gauge sends ahead of the data.
    ///
    /// The header is the count byte for a block register, or the count byte and the echoed subcommand for
    /// a MAC response read from `ManufacturerBlockAccess()`.
    async fn read_block(
        &mut self,
        command: u8,
        header_len: usize,
        read: &mut [u8],
    ) -> Result<(), Bq41z50Error<BUS::Error>> {
        if read.len() > LARGEST_CMD_BYTES {
            return Err(Bq41z50Error::DataTooLarge);
        }

        let mut buf = [0u8; MAC_CMD_RX_HEADER_SIZE_BYTES + LARGEST_CMD_BYTES];
        let response_len = header_len + read.len();

        self.bus
            .write_read(BQ41Z50_ADDR, &[command], &mut buf[..response_len])
            .await
            .map_err(Bq41z50Error::Bus)?;
        read.copy_from_slice(&buf[header_len..response_len]);
        Ok(())
    }

    /// Read an `SMBus` block (a count byte followed by up to 32 bytes) into a NUL-terminated string.
    ///
    /// The count byte is dropped and the string is truncated to fit `out` with its terminator; the rest of
    /// `out` is zero-filled.
    async fn read_block_string(&mut self, command: u8, out: &mut [u8]) -> Result<(), Bq41z50Error<BUS::Error>> {
        if out.is_empty() {
            return Ok(());
        }

        let mut buf = [0u8; SMBUS_BLOCK_COUNT_SIZE_BYTES + LARGEST_BUF_BYTES];
        self.bus
            .write_read(BQ41Z50_ADDR, &[command], &mut buf)
            .await
            .map_err(Bq41z50Error::Bus)?;

        let len = usize::from(buf[0]).min(LARGEST_BUF_BYTES).min(out.len() - 1);
        out[..len].copy_from_slice(&buf[SMBUS_BLOCK_COUNT_SIZE_BYTES..SMBUS_BLOCK_COUNT_SIZE_BYTES + len]);
        out[len..].fill(0);
        Ok(())
    }
}

impl<BUS: embedded_hal_async::i2c::I2c> device_driver::RegisterInterfaceBase for Interface<BUS> {
    type Error = Bq41z50Error<BUS::Error>;

    type AddressType = u8;
}

impl<BUS: embedded_hal_async::i2c::I2c> device_driver::AsyncRegisterInterface for Interface<BUS> {
    async fn write_register(
        &mut self,
        address: Self::AddressType,
        data: &mut [u8],
        _metadata: &FieldsetMetadata,
    ) -> Result<(), Self::Error> {
        // Build a single I2C write frame: [register_address, data...]
        let mut buf = [0u8; LARGEST_REG_BYTES + SBS_ADDR_REG_SIZE];

        // Place the SBS register address in the first byte of the buffer
        buf[..SBS_ADDR_REG_SIZE].copy_from_slice(&address.to_le_bytes());
        // Append the register payload immediately after the address
        buf[SBS_ADDR_REG_SIZE..data.len() + SBS_ADDR_REG_SIZE].copy_from_slice(data);
        // Transmit the combined address + data as a single I2C write to the device
        self.bus
            .write(BQ41Z50_ADDR, &buf[..data.len() + SBS_ADDR_REG_SIZE])
            .await
            .map_err(Bq41z50Error::Bus)
    }

    async fn read_register(
        &mut self,
        address: Self::AddressType,
        data: &mut [u8],
        _metadata: &FieldsetMetadata,
    ) -> Result<(), Self::Error> {
        // Send the register address and read back the register contents
        self.bus
            .write_read(BQ41Z50_ADDR, &[address], data)
            .await
            .map_err(Bq41z50Error::Bus)
    }
}

impl<BUS: embedded_hal_async::i2c::I2c> device_driver::CommandInterfaceBase for Interface<BUS> {
    type Error = Bq41z50Error<BUS::Error>;

    type AddressType = u32;
}

impl<BUS: embedded_hal_async::i2c::I2c> device_driver::AsyncCommandInterface for Interface<BUS> {
    async fn dispatch_command(
        &mut self,
        address: Self::AddressType,
        _input: &mut [u8],
        _metadata_in: &FieldsetMetadata,
        output: &mut [u8],
        _metadata_out: &FieldsetMetadata,
    ) -> Result<(), Self::Error> {
        // Command addresses are 0x00_CC_SSSS: the SBS command, then the subcommand (little-endian).
        let [_, sbs_command, subcommand_lo, subcommand_hi] = address.to_be_bytes();

        if sbs_command != MAC_CMD_SBS_ADDR {
            // An SMBus block register: no subcommand, just a block read with a count byte ahead of the data
            return self.read_block(sbs_command, SMBUS_BLOCK_COUNT_SIZE_BYTES, output).await;
        }

        let mac_command = u16::from_le_bytes([subcommand_lo, subcommand_hi]);
        if output.is_empty() {
            self.mac_send(mac_command).await
        } else {
            self.mac_read(mac_command, output).await
        }
    }
}

impl<BUS: embedded_hal_async::i2c::I2c> device_driver::BufferInterfaceBase for Interface<BUS> {
    type Error = Bq41z50Error<BUS::Error>;

    type AddressType = u8;
}

impl<BUS: embedded_hal_async::i2c::I2c> device_driver::AsyncBufferInterface for Interface<BUS> {
    async fn read(&mut self, address: Self::AddressType, buf: &mut [u8]) -> Result<usize, Self::Error> {
        // Send the register address and read back the register contents
        self.bus
            .write_read(BQ41Z50_ADDR, &[address], buf)
            .await
            .map_err(Bq41z50Error::Bus)
            .map(|_| buf.len())
    }

    async fn write(&mut self, address: Self::AddressType, buf: &[u8]) -> Result<usize, Self::Error> {
        let mut temp_buf = [0u8; LARGEST_BUF_BYTES + SBS_ADDR_REG_SIZE];

        // Place the SBS register address in the first byte of the buffer
        temp_buf[..SBS_ADDR_REG_SIZE].copy_from_slice(&address.to_le_bytes());
        // Append the register payload immediately after the address
        temp_buf[SBS_ADDR_REG_SIZE..buf.len() + SBS_ADDR_REG_SIZE].copy_from_slice(buf);
        // Transmit the combined address + data as a single I2C write to the device
        self.bus
            .write(BQ41Z50_ADDR, &temp_buf[..buf.len() + SBS_ADDR_REG_SIZE])
            .await
            .map_err(Bq41z50Error::Bus)
            .map(|_| buf.len())
    }

    async fn flush(&mut self, _address: Self::AddressType) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum CapacityModeState {
    Milliamps = 0,
    Centiwatt = 1,
}

/// A temperature in centi-degrees Celsius (0.01 C units).
///
/// Celsius and Fahrenheit values are distinct types and cannot be interchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CentiCelsius(i32);

impl CentiCelsius {
    /// Return the temperature in centi-degrees Celsius (0.01 C units).
    #[must_use]
    pub const fn centi_degrees(self) -> i32 {
        self.0
    }

    /// Return the temperature in degrees Celsius.
    #[must_use]
    pub fn to_degrees(self) -> f64 {
        f64::from(self.0) / 100.0
    }
}

/// A temperature in centi-degrees Fahrenheit (0.01 F units).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CentiFahrenheit(i32);

impl CentiFahrenheit {
    /// Return the temperature in centi-degrees Fahrenheit (0.01 F units).
    #[must_use]
    pub const fn centi_degrees(self) -> i32 {
        self.0
    }

    /// Return the temperature in degrees Fahrenheit.
    #[must_use]
    pub fn to_degrees(self) -> f64 {
        f64::from(self.0) / 100.0
    }
}

fn decikelvin_to_centi_celsius(decikelvin: smart_battery::DeciKelvin) -> CentiCelsius {
    CentiCelsius(i32::from(decikelvin) * 10 - 27_315)
}

fn decikelvin_to_centi_fahrenheit(decikelvin: smart_battery::DeciKelvin) -> CentiFahrenheit {
    CentiFahrenheit(decikelvin_to_centi_celsius(decikelvin).centi_degrees() * 9 / 5 + 3_200)
}

#[derive(Debug)]
pub struct Bq41z50<BUS: embedded_hal_async::i2c::I2c> {
    pub device: device::Device<Interface<BUS>>,
    capacity_mode_state: CapacityModeState,
}

impl<BUS: embedded_hal_async::i2c::I2c> Bq41z50<BUS> {
    pub fn new(bus: BUS) -> Self {
        Self {
            device: device::Device::new(Interface::new(bus)),
            capacity_mode_state: CapacityModeState::Milliamps,
        }
    }

    fn set_capacity_mode_state(&mut self, battery_mode_fields: smart_battery::BatteryModeFields) {
        self.capacity_mode_state = if battery_mode_fields.capacity_mode() {
            CapacityModeState::Centiwatt
        } else {
            CapacityModeState::Milliamps
        };
    }

    /// Read the battery temperature and return it in centi-degrees Celsius.
    ///
    /// The gauge reports temperature in decikelvin (0.1 K units); this reads the
    /// `Temperature()` register and converts the value to 0.01 C units.
    ///
    /// # Errors
    ///
    /// Will return `Err` if an I2C bus error occurs.
    pub async fn temperature_celsius(&mut self) -> Result<CentiCelsius, Bq41z50Error<BUS::Error>> {
        let decikelvin = self.device.temperature().read_async().await?.temperature();
        Ok(decikelvin_to_centi_celsius(decikelvin))
    }

    /// Read the battery temperature and return it in centi-degrees Fahrenheit.
    ///
    /// The gauge reports temperature in decikelvin (0.1 K units); this reads the
    /// `Temperature()` register and converts the value to 0.01 F units.
    ///
    /// # Errors
    ///
    /// Will return `Err` if an I2C bus error occurs.
    pub async fn temperature_fahrenheit(&mut self) -> Result<CentiFahrenheit, Bq41z50Error<BUS::Error>> {
        let decikelvin = self.device.temperature().read_async().await?.temperature();
        Ok(decikelvin_to_centi_fahrenheit(decikelvin))
    }

    /// Read MAC Register 0x0035 Security Keys.
    ///
    /// This function has special functionality compared to the rest of the MAC commands and so it is handled in its
    /// own function.
    ///
    /// # Errors
    ///
    /// Will return `Err` if an I2C bus error occurs.
    pub async fn read_security_keys(
        &mut self,
        output_buf: &mut [u8; SECURITY_KEYS_DATA_LEN_BYTES as usize],
    ) -> Result<(), Bq41z50Error<BUS::Error>> {
        self.device
            .interface()
            .mac_read(u16::from_le_bytes(SECURITY_KEYS_CMD), output_buf)
            .await
    }

    /// Write MAC Register 0x0035 Security Keys.
    ///
    /// This function has special functionality compared to the rest of the MAC commands and so it is handled in its
    /// own function.
    ///
    /// # Errors
    ///
    /// Will return `Err` if an I2C bus error occurs.
    pub async fn write_security_keys(
        &mut self,
        security_keys: &[u8; SECURITY_KEYS_DATA_LEN_BYTES as usize],
    ) -> Result<(), Bq41z50Error<BUS::Error>> {
        let mut buf = [0u8; 2 + MAC_CMD_ADDR_SIZE_BYTES as usize + SECURITY_KEYS_DATA_LEN_BYTES as usize];
        buf[0] = MAC_CMD_SBS_ADDR;
        buf[1] = SECURITY_KEYS_LEN_BYTES;
        buf[2] = SECURITY_KEYS_CMD[0];
        buf[3] = SECURITY_KEYS_CMD[1];
        buf[4..].copy_from_slice(security_keys);

        self.device
            .interface()
            .bus
            .write(BQ41Z50_ADDR, &buf)
            .await
            .map_err(Bq41z50Error::Bus)
    }

    /// Read MAC Register 0x0037 Authentication Key.
    ///
    /// This function has special functionality compared to the rest of the MAC commands and so it is handled in its
    /// own function.
    ///
    /// # Errors
    ///
    /// Will return `Err` if an I2C bus error occurs.
    pub async fn read_authentication_key(
        &mut self,
        output_buf: &mut [u8; AUTH_KEY_DATA_LEN_BYTES as usize],
    ) -> Result<(), Bq41z50Error<BUS::Error>> {
        self.device
            .interface()
            .mac_read(u16::from_le_bytes(AUTH_KEY_CMD), output_buf)
            .await
    }

    /// Write MAC Register 0x0037 Authentication Key.
    ///
    /// This function has special functionality compared to the rest of the MAC commands and so it is handled in its
    /// own function.
    ///
    /// # Errors
    ///
    /// Will return `Err` if an I2C bus error occurs.
    pub async fn write_authentication_key(
        &mut self,
        auth_key: &[u8; AUTH_KEY_DATA_LEN_BYTES as usize],
    ) -> Result<(), Bq41z50Error<BUS::Error>> {
        let mut buf = [0u8; 2 + MAC_CMD_ADDR_SIZE_BYTES as usize + AUTH_KEY_DATA_LEN_BYTES as usize];
        buf[0] = MAC_CMD_SBS_ADDR;
        buf[1] = AUTH_KEY_LEN_BYTES;
        buf[2] = AUTH_KEY_CMD[0];
        buf[3] = AUTH_KEY_CMD[1];
        buf[4..].copy_from_slice(auth_key);

        self.device
            .interface()
            .bus
            .write(BQ41Z50_ADDR, &buf)
            .await
            .map_err(Bq41z50Error::Bus)
    }

    /// Read MAC 0x0004 `InstructionFlashSignature()`.
    ///
    /// The gauge NACKs the read while it computes the signature, so this sends the command, waits 500 ms on
    /// `delay` and then reads it. Use this instead of `device.mac_instruction_flash_signature()`.
    ///
    /// # Errors
    ///
    /// Will return `Err` if an I2C bus error occurs.
    pub async fn read_instruction_flash_signature(
        &mut self,
        delay: &mut impl DelayNs,
    ) -> Result<device::MacInstructionFlashSignatureFieldsOut, Bq41z50Error<BUS::Error>> {
        let mut data = [0u8; 2];
        self.read_slow_mac(
            INSTRUCTION_FLASH_SIGNATURE_CMD,
            INSTRUCTION_FLASH_SIGNATURE_DELAY_MS,
            delay,
            &mut data,
        )
        .await?;
        Ok(data.into())
    }

    /// Read MAC 0x0005 `StaticDFSignature()`.
    ///
    /// The gauge NACKs the read while it computes the signature, so this sends the command, waits 1 s on
    /// `delay` and then reads it. Use this instead of `device.mac_static_df_signature()`.
    ///
    /// # Errors
    ///
    /// Will return `Err` if an I2C bus error occurs.
    pub async fn read_static_df_signature(
        &mut self,
        delay: &mut impl DelayNs,
    ) -> Result<device::MacStaticDfSignatureFieldsOut, Bq41z50Error<BUS::Error>> {
        let mut data = [0u8; 2];
        self.read_slow_mac(STATIC_DF_SIGNATURE_CMD, STATIC_DF_SIGNATURE_DELAY_MS, delay, &mut data)
            .await?;
        Ok(data.into())
    }

    /// Read MAC 0x0009 `AllDFSignature()`.
    ///
    /// The gauge NACKs the read while it computes the signature, so this sends the command, waits 1 s on
    /// `delay` and then reads it. Use this instead of `device.mac_all_df_signature()`.
    ///
    /// # Errors
    ///
    /// Will return `Err` if an I2C bus error occurs.
    pub async fn read_all_df_signature(
        &mut self,
        delay: &mut impl DelayNs,
    ) -> Result<device::MacStaticChemDfSigFieldsOut, Bq41z50Error<BUS::Error>> {
        let mut data = [0u8; 2];
        self.read_slow_mac(ALL_DF_SIGNATURE_CMD, ALL_DF_SIGNATURE_DELAY_MS, delay, &mut data)
            .await?;
        Ok(data.into())
    }

    async fn read_slow_mac(
        &mut self,
        mac_command: u16,
        delay_ms: u32,
        delay: &mut impl DelayNs,
        read: &mut [u8],
    ) -> Result<(), Bq41z50Error<BUS::Error>> {
        let interface = self.device.interface();
        interface.mac_send(mac_command).await?;
        delay.delay_ms(delay_ms).await;
        interface
            .read_block(MAC_CMD_SBS_ADDR, MAC_CMD_RX_HEADER_SIZE_BYTES, read)
            .await
    }

    /// Seal the fuel gauge.
    /// # Errors
    ///
    /// Will return `Err` if an I2C bus error occurs.
    pub async fn seal_fg(&mut self) -> Result<(), Bq41z50Error<BUS::Error>> {
        self.device.mac_seal().dispatch_async().await
    }

    /// Unseal the fuel gauge.
    ///
    /// # Errors
    ///
    /// Will return `Err` if an I2C bus error occurs.
    pub async fn unseal_fg(
        &mut self,
        unseal_key_lower: u16,
        unseal_key_upper: u16,
    ) -> Result<(), Bq41z50Error<BUS::Error>> {
        self.send_access_key(unseal_key_lower, unseal_key_upper).await
    }

    /// Send access keys.
    ///
    /// Various keys are defined, please check the datasheet of the revision you're working with for
    /// the types of keys and their function.
    ///
    /// # Errors
    ///
    /// Will return `Err` if an I2C bus error occurs.
    pub async fn send_access_key(
        &mut self,
        access_key_lower: u16,
        access_key_upper: u16,
    ) -> Result<(), Bq41z50Error<BUS::Error>> {
        let mut buf = [0u8; 4];

        // Write lower access key
        buf[0] = MAC_CMD_SBS_ADDR;
        buf[1] = MAC_CMD_ADDR_SIZE_BYTES;
        buf[2..4].copy_from_slice(&access_key_lower.to_le_bytes());
        self.device
            .interface()
            .bus
            .write(BQ41Z50_ADDR, &buf)
            .await
            .map_err(Bq41z50Error::Bus)?;

        // Write upper access key
        buf[0] = MAC_CMD_SBS_ADDR;
        buf[1] = MAC_CMD_ADDR_SIZE_BYTES;
        buf[2..4].copy_from_slice(&access_key_upper.to_le_bytes());
        self.device
            .interface()
            .bus
            .write(BQ41Z50_ADDR, &buf)
            .await
            .map_err(Bq41z50Error::Bus)
    }

    /// Write to `MfgInfoC` MAC register.
    ///
    /// `data` can be at most 32 bytes large.
    ///
    /// For the R5 revision, the `MfgInfoC` access keys are required to be passed in as arguments.
    /// # Errors
    ///
    /// Will return `Err` if an I2C bus error occurs or `data` exceeds 32 bytes.
    #[allow(clippy::cast_possible_truncation)]
    pub async fn write_mfg_info_c(
        &mut self,
        access_key_lower: u16,
        access_key_upper: u16,
        data: &[u8],
    ) -> Result<(), Bq41z50Error<BUS::Error>> {
        if data.len() > LARGEST_CMD_BYTES {
            return Err(Bq41z50Error::DataTooLarge);
        }
        let mut buf = [0u8; 4 + LARGEST_CMD_BYTES];

        self.send_access_key(access_key_lower, access_key_upper).await?;

        // Write data
        buf[0] = MAC_CMD_SBS_ADDR;
        buf[1] = data.len() as u8 + MAC_CMD_ADDR_SIZE_BYTES;
        buf[2] = MFG_INFO_C_CMD[0];
        buf[3] = MFG_INFO_C_CMD[1];
        buf[4..data.len() + 4].copy_from_slice(data);

        self.device
            .interface()
            .bus
            .write(BQ41Z50_ADDR, &buf[..data.len() + 4])
            .await
            .map_err(Bq41z50Error::Bus)
    }

    /// Read from the `MfgInfoC` MAC register.
    ///
    /// `data` can be at most 32 bytes large.
    /// # Errors
    ///
    /// Will return `Err` if an I2C bus error occurs or `data` exceeds 32 bytes.
    pub async fn read_mfg_info_c(&mut self, data: &mut [u8]) -> Result<(), Bq41z50Error<BUS::Error>> {
        if data.len() > LARGEST_CMD_BYTES {
            return Err(Bq41z50Error::DataTooLarge);
        }

        self.device
            .interface()
            .mac_read(u16::from_le_bytes(MFG_INFO_C_CMD), data)
            .await
    }

    /// Write to the `MfgInfo` register. Despite it not being a MAC cmd, it uses the `SMBus` block command.
    ///
    /// Requires fuel gauge to be unsealed. Send `unseal_fg()` first, and then reseal with `seal_fg()` after this command.
    ///
    /// `data` can be at most 32 bytes large.
    /// # Errors
    ///
    /// Will return `Err` if an I2C bus error occurs or `data` exceeds 32 bytes.
    #[allow(clippy::cast_possible_truncation)]
    pub async fn write_mfg_info(&mut self, data: &[u8]) -> Result<(), Bq41z50Error<BUS::Error>> {
        if data.len() > 32 {
            return Err(Bq41z50Error::DataTooLarge);
        }
        let mut buf = [0u8; 2 + LARGEST_REG_BYTES];
        buf[0] = MFG_INFO_CMD;
        buf[1] = data.len() as u8;
        buf[2..data.len() + 2].copy_from_slice(data);

        self.device
            .interface()
            .bus
            .write(BQ41Z50_ADDR, &buf[..data.len() + 2])
            .await
            .map_err(Bq41z50Error::Bus)
    }

    /// Read from the `MfgInfo` register.
    ///
    /// NOTE: `mfg_info` is unique in that it has a leading size byte,
    /// meaning you need to read 33 bytes to read 32 bytes of data.
    /// This fn will return the data including the size byte as the zeroth byte.
    ///
    /// `data` can be at most 33 bytes large, which includes the size byte in the zeroth position.
    /// # Errors
    ///
    /// Will return `Err` if an I2C bus error occurs or `data` exceeds 33 bytes.
    pub async fn read_mfg_info(&mut self, data: &mut [u8]) -> Result<(), Bq41z50Error<BUS::Error>> {
        if data.len() > 33 {
            return Err(Bq41z50Error::DataTooLarge);
        }

        self.device
            .interface()
            .bus
            .write_read(BQ41Z50_ADDR, &[MFG_INFO_CMD], data)
            .await
            .map_err(Bq41z50Error::Bus)
    }

    /// Write to the `ChargingVoltageOverride` MAC Command.
    /// # Errors
    ///
    /// Will return `Err` if an I2C bus error occurs.
    pub async fn write_charging_voltage_override(
        &mut self,
        override_struct: &ChargingVoltageOverride,
    ) -> Result<(), Bq41z50Error<BUS::Error>> {
        let mut buf = [0u8; 4 + CHRG_VOLTAGE_OVERRIDE_SIZE_BYTES as usize];

        buf[0] = MAC_CMD_SBS_ADDR;
        buf[1] = CHRG_VOLTAGE_OVERRIDE_SIZE_BYTES + MAC_CMD_ADDR_SIZE_BYTES;
        buf[2] = CHRG_VOLTAGE_OVERRIDE_CMD[0];
        buf[3] = CHRG_VOLTAGE_OVERRIDE_CMD[1];
        buf[4..6].copy_from_slice(&override_struct.low_temp_chrg_mv.to_le_bytes());
        buf[6..8].copy_from_slice(&override_struct.std_low_temp_chrg_mv.to_le_bytes());
        buf[8..10].copy_from_slice(&override_struct.std_hi_temp_chrg_mv.to_le_bytes());
        buf[10..12].copy_from_slice(&override_struct.hi_temp_chrg_mv.to_le_bytes());
        buf[12..14].copy_from_slice(&override_struct.recommended_temp_chrg_mv.to_le_bytes());

        self.device
            .interface()
            .bus
            .write(BQ41Z50_ADDR, &buf)
            .await
            .map_err(Bq41z50Error::Bus)
    }

    /// Read from the `ChargingVoltageOverride` register.
    /// # Errors
    ///
    /// Will return `Err` if an I2C bus error occurs.
    /// # Panics
    /// Safe from Panics as the internal buffer is guaranteed to be large enough (10 bytes).
    pub async fn read_charging_voltage_override(
        &mut self,
    ) -> Result<ChargingVoltageOverride, Bq41z50Error<BUS::Error>> {
        let mut data = [0u8; CHRG_VOLTAGE_OVERRIDE_SIZE_BYTES as usize];

        self.device
            .interface()
            .mac_read(u16::from_le_bytes(CHRG_VOLTAGE_OVERRIDE_CMD), &mut data)
            .await?;

        // Safe from Panics as the buffer is guaranteed to be large enough (10 bytes).
        Ok(ChargingVoltageOverride {
            low_temp_chrg_mv: u16::from_le_bytes(data[0..2].try_into().unwrap()),
            std_low_temp_chrg_mv: u16::from_le_bytes(data[2..4].try_into().unwrap()),
            std_hi_temp_chrg_mv: u16::from_le_bytes(data[4..6].try_into().unwrap()),
            hi_temp_chrg_mv: u16::from_le_bytes(data[6..8].try_into().unwrap()),
            recommended_temp_chrg_mv: u16::from_le_bytes(data[8..10].try_into().unwrap()),
        })
    }
}
