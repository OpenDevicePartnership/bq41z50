use crate::Bq41z50;
use crate::Bq41z50Error;
use crate::CapacityModeState;
use crate::device;
use device_driver::Block;
use embedded_batteries_async::smart_battery;

// SBS string commands. These are `SMBus` block reads: a count byte followed by the text.
const MANUFACTURER_NAME_CMD: u8 = 0x20;
const DEVICE_NAME_CMD: u8 = 0x21;
const DEVICE_CHEMISTRY_CMD: u8 = 0x22;

impl From<device::BatteryStatus> for smart_battery::BatteryStatusFields {
    fn from(value: device::BatteryStatus) -> Self {
        smart_battery::BatteryStatusFields::new()
            .with_error_code(value.ec())
            .with_fully_discharged(value.fd())
            .with_fully_charged(value.fc())
            .with_discharging(value.dsg())
            .with_initialized(value.init())
            .with_remaining_time_alarm(value.rta())
            .with_remaining_capacity_alarm(value.rca())
            .with_terminate_discharge_alarm(value.tda())
            .with_over_temp_alarm(value.ota())
            .with_terminate_charge_alarm(value.tca())
            .with_over_charged_alarm(value.oca())
    }
}

impl From<device::BatteryMode> for smart_battery::BatteryModeFields {
    fn from(value: device::BatteryMode) -> Self {
        let battery_mode_raw: [u8; 2] = value.into();
        u16::from_le_bytes(battery_mode_raw).into()
    }
}

impl From<smart_battery::BatteryModeFields> for device::BatteryMode {
    fn from(value: smart_battery::BatteryModeFields) -> Self {
        u16::from(value).to_le_bytes().into()
    }
}

impl From<device::SpecificationInfo> for smart_battery::SpecificationInfoFields {
    fn from(value: device::SpecificationInfo) -> Self {
        let spec_info_raw: [u8; 2] = value.into();
        u16::from_le_bytes(spec_info_raw).into()
    }
}

impl<BUS: embedded_hal_async::i2c::I2c> embedded_batteries_async::smart_battery::ErrorType for Bq41z50<BUS> {
    type Error = Bq41z50Error<BUS::Error>;
}

impl<BUS: embedded_hal_async::i2c::I2c> embedded_batteries_async::smart_battery::SmartBattery for Bq41z50<BUS> {
    async fn remaining_capacity_alarm(&mut self) -> Result<smart_battery::CapacityModeValue, Self::Error> {
        Ok(match self.capacity_mode_state {
            CapacityModeState::Milliamps => smart_battery::CapacityModeValue::MilliAmpUnsigned(
                self.device
                    .remaining_capacity_alarm()
                    .read_async()
                    .await?
                    .remaining_capacity_alarm(),
            ),
            CapacityModeState::Centiwatt => smart_battery::CapacityModeValue::CentiWattUnsigned(
                self.device
                    .remaining_capacity_alarm()
                    .read_async()
                    .await?
                    .remaining_capacity_alarm(),
            ),
        })
    }

    async fn set_remaining_capacity_alarm(
        &mut self,
        capacity: smart_battery::CapacityModeValue,
    ) -> Result<(), Self::Error> {
        self.device
            .remaining_capacity_alarm()
            .write_async(|d| {
                d.set_remaining_capacity_alarm(match capacity {
                    smart_battery::CapacityModeValue::MilliAmpUnsigned(value)
                    | smart_battery::CapacityModeValue::CentiWattUnsigned(value) => value,
                });
            })
            .await
    }

    async fn remaining_time_alarm(&mut self) -> Result<smart_battery::Minutes, Self::Error> {
        Ok(self
            .device
            .remaining_time_alarm()
            .read_async()
            .await?
            .remaining_time_alarm())
    }

    async fn set_remaining_time_alarm(&mut self, time: smart_battery::Minutes) -> Result<(), Self::Error> {
        self.device
            .remaining_time_alarm()
            .write_async(|f| f.set_remaining_time_alarm(time))
            .await
    }

    async fn battery_mode(&mut self) -> Result<smart_battery::BatteryModeFields, Self::Error> {
        Ok(self.device.battery_mode().read_async().await?.into())
    }

    async fn set_battery_mode(&mut self, flags: smart_battery::BatteryModeFields) -> Result<(), Self::Error> {
        self.set_capacity_mode_state(flags);
        self.device.battery_mode().write_async(|f| *f = flags.into()).await
    }

    async fn at_rate(&mut self) -> Result<smart_battery::CapacityModeSignedValue, Self::Error> {
        Ok(match self.capacity_mode_state {
            CapacityModeState::Milliamps => smart_battery::CapacityModeSignedValue::MilliAmpSigned(
                self.device.at_rate().read_async().await?.at_rate(),
            ),
            CapacityModeState::Centiwatt => smart_battery::CapacityModeSignedValue::CentiWattSigned(
                self.device.at_rate().read_async().await?.at_rate(),
            ),
        })
    }

    async fn set_at_rate(&mut self, rate: smart_battery::CapacityModeSignedValue) -> Result<(), Self::Error> {
        self.device
            .at_rate()
            .write_async(|f| {
                f.set_at_rate(match rate {
                    smart_battery::CapacityModeSignedValue::MilliAmpSigned(value)
                    | smart_battery::CapacityModeSignedValue::CentiWattSigned(value) => value,
                });
            })
            .await
    }

    async fn at_rate_time_to_full(&mut self) -> Result<smart_battery::Minutes, Self::Error> {
        Ok(self
            .device
            .at_rate_time_to_full()
            .read_async()
            .await?
            .at_rate_time_to_full())
    }

    async fn at_rate_time_to_empty(&mut self) -> Result<smart_battery::Minutes, Self::Error> {
        Ok(self
            .device
            .at_rate_time_to_empty()
            .read_async()
            .await?
            .at_rate_time_to_empty())
    }

    async fn at_rate_ok(&mut self) -> Result<bool, Self::Error> {
        // 0 = false, anything else is true
        Ok(self.device.at_rate_ok().read_async().await?.at_rate_ok() != 0)
    }

    async fn temperature(&mut self) -> Result<embedded_batteries_async::smart_battery::DeciKelvin, Self::Error> {
        Ok(self.device.temperature().read_async().await?.temperature())
    }

    async fn voltage(&mut self) -> Result<smart_battery::MilliVolts, Self::Error> {
        Ok(self.device.voltage().read_async().await?.voltage())
    }

    async fn current(&mut self) -> Result<smart_battery::MilliAmpsSigned, Self::Error> {
        Ok(self.device.current().read_async().await?.current())
    }

    async fn average_current(&mut self) -> Result<smart_battery::MilliAmpsSigned, Self::Error> {
        Ok(self.device.avg_current().read_async().await?.avg_current())
    }

    #[allow(clippy::cast_possible_truncation)]
    async fn max_error(&mut self) -> Result<smart_battery::Percent, Self::Error> {
        // Even though the datasheet of the fuel gauge says the data is 1 byte, through PEC testing the fuel gauge
        // actually returns 2 bytes. The range of the data is 0 - 100 so it will never exceed 1 byte, but in order to
        // receive the PEC byte we need to read 2 bytes of data.
        Ok(self.device.max_error().read_async().await?.max_error() as u8)
    }

    #[allow(clippy::cast_possible_truncation)]
    async fn relative_state_of_charge(&mut self) -> Result<smart_battery::Percent, Self::Error> {
        // Even though the datasheet of the fuel gauge says the data is 1 byte, through PEC testing the fuel gauge
        // actually returns 2 bytes. The range of the data is 0 - 100 so it will never exceed 1 byte, but in order to
        // receive the PEC byte we need to read 2 bytes of data.
        Ok(self
            .device
            .relative_state_of_charge()
            .read_async()
            .await?
            .relative_state_of_charge() as u8)
    }

    #[allow(clippy::cast_possible_truncation)]
    async fn absolute_state_of_charge(&mut self) -> Result<smart_battery::Percent, Self::Error> {
        // Even though the datasheet of the fuel gauge says the data is 1 byte, through PEC testing the fuel gauge
        // actually returns 2 bytes. The range of the data is 0 - 100 so it will never exceed 1 byte, but in order to
        // receive the PEC byte we need to read 2 bytes of data.
        Ok(self
            .device
            .absolute_state_of_charge()
            .read_async()
            .await?
            .absolute_state_of_charge() as u8)
    }

    async fn remaining_capacity(&mut self) -> Result<smart_battery::CapacityModeValue, Self::Error> {
        Ok(match self.capacity_mode_state {
            CapacityModeState::Milliamps => smart_battery::CapacityModeValue::MilliAmpUnsigned(
                self.device
                    .remaining_capacity()
                    .read_async()
                    .await?
                    .remaining_capacity(),
            ),
            CapacityModeState::Centiwatt => smart_battery::CapacityModeValue::CentiWattUnsigned(
                self.device
                    .remaining_capacity()
                    .read_async()
                    .await?
                    .remaining_capacity(),
            ),
        })
    }

    async fn full_charge_capacity(&mut self) -> Result<smart_battery::CapacityModeValue, Self::Error> {
        Ok(match self.capacity_mode_state {
            CapacityModeState::Milliamps => smart_battery::CapacityModeValue::MilliAmpUnsigned(
                self.device
                    .full_charge_capacity()
                    .read_async()
                    .await?
                    .full_charge_capacity(),
            ),
            CapacityModeState::Centiwatt => smart_battery::CapacityModeValue::CentiWattUnsigned(
                self.device
                    .full_charge_capacity()
                    .read_async()
                    .await?
                    .full_charge_capacity(),
            ),
        })
    }

    async fn run_time_to_empty(&mut self) -> Result<smart_battery::Minutes, Self::Error> {
        Ok(self.device.run_time_to_empty().read_async().await?.run_time_to_empty())
    }

    async fn average_time_to_empty(&mut self) -> Result<smart_battery::Minutes, Self::Error> {
        Ok(self
            .device
            .average_time_to_empty()
            .read_async()
            .await?
            .average_time_to_empty())
    }

    async fn average_time_to_full(&mut self) -> Result<smart_battery::Minutes, Self::Error> {
        Ok(self
            .device
            .average_time_to_full()
            .read_async()
            .await?
            .average_time_to_full())
    }

    async fn battery_status(&mut self) -> Result<smart_battery::BatteryStatusFields, Self::Error> {
        // The error code bits report the result of the previous SBS command, not a battery fault, so they are
        // returned to the caller in the status rather than as an error.
        Ok(self.device.battery_status().read_async().await?.into())
    }

    async fn cycle_count(&mut self) -> Result<smart_battery::Cycles, Self::Error> {
        Ok(self.device.cycle_count().read_async().await?.cycle_count())
    }

    async fn design_capacity(&mut self) -> Result<smart_battery::CapacityModeValue, Self::Error> {
        Ok(match self.capacity_mode_state {
            CapacityModeState::Milliamps => smart_battery::CapacityModeValue::MilliAmpUnsigned(
                self.device.design_capacity().read_async().await?.design_capacity(),
            ),
            CapacityModeState::Centiwatt => smart_battery::CapacityModeValue::CentiWattUnsigned(
                self.device.design_capacity().read_async().await?.design_capacity(),
            ),
        })
    }

    async fn design_voltage(&mut self) -> Result<smart_battery::MilliVolts, Self::Error> {
        Ok(self.device.design_voltage().read_async().await?.design_voltage())
    }

    async fn specification_info(&mut self) -> Result<smart_battery::SpecificationInfoFields, Self::Error> {
        Ok(self.device.specification_info().read_async().await?.into())
    }

    async fn manufacture_date(&mut self) -> Result<smart_battery::ManufactureDate, Self::Error> {
        Ok(self
            .device
            .manufacture_date()
            .read_async()
            .await?
            .manufacture_date()
            .into())
    }

    async fn serial_number(&mut self) -> Result<u16, Self::Error> {
        Ok(self.device.serial_number().read_async().await?.serial_number())
    }

    async fn manufacturer_name(&mut self, name: &mut [u8]) -> Result<(), Self::Error> {
        self.device
            .interface()
            .read_block_string(MANUFACTURER_NAME_CMD, name)
            .await
    }

    async fn device_name(&mut self, name: &mut [u8]) -> Result<(), Self::Error> {
        self.device.interface().read_block_string(DEVICE_NAME_CMD, name).await
    }

    async fn device_chemistry(&mut self, chemistry: &mut [u8]) -> Result<(), Self::Error> {
        self.device
            .interface()
            .read_block_string(DEVICE_CHEMISTRY_CMD, chemistry)
            .await
    }

    async fn charging_current(&mut self) -> Result<embedded_batteries_async::charger::MilliAmps, Self::Error> {
        Ok(self.device.charging_current().read_async().await?.charging_current())
    }

    async fn charging_voltage(&mut self) -> Result<smart_battery::MilliVolts, Self::Error> {
        Ok(self.device.charging_voltage().read_async().await?.charging_voltage())
    }
}
