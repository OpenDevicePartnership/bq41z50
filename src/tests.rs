use crate::*;
use embedded_hal_async::i2c::ErrorKind;
use embedded_hal_mock::eh1::i2c::Mock;

#[tokio::test]
async fn construct_interface() {
    let expectations = vec![
        embedded_hal_mock::eh1::i2c::Transaction::write_read(crate::BQ41Z50_ADDR, vec![0x08], vec![0xFF, 0x00]),
        embedded_hal_mock::eh1::i2c::Transaction::write(crate::BQ41Z50_ADDR, vec![0x4A, 0x05, 0x00]),
        embedded_hal_mock::eh1::i2c::Transaction::write(crate::BQ41Z50_ADDR, vec![0x1C, 0x43, 0x00]),
    ];

    let mut device = crate::device::Device::new(crate::Interface {
        bus: embedded_hal_mock::eh1::i2c::Mock::new(&expectations),
    });

    let temp = device.temperature().read_async().await.unwrap();

    assert_eq!(temp.temperature(), 0x00FF);

    device
        .btp_discharge_set()
        .write_async(|f| f.set_btp_discharge_set(5))
        .await
        .unwrap();

    device
        .serial_number()
        .write_async(|f| f.set_serial_number(67))
        .await
        .unwrap();

    device.interface().bus.done();
}

#[tokio::test]
async fn temperature_conversions_preserve_units_and_scale() {
    for (decikelvin, centi_celsius, centi_fahrenheit) in [
        (0u16, -27_315, -45_967),
        (2_332, -3_995, -3_991),
        (2_731, -5, 3_191),
        (2_732, 5, 3_209),
        (2_982, 2_505, 7_709),
        (u16::MAX, 628_035, 1_133_663),
    ] {
        let transaction = embedded_hal_mock::eh1::i2c::Transaction::write_read(
            crate::BQ41Z50_ADDR,
            vec![0x08],
            decikelvin.to_le_bytes().to_vec(),
        );
        let expectations = [transaction.clone(), transaction];
        let mut driver = Bq41z50::new(Mock::new(&expectations));

        let celsius: CentiCelsius = driver.temperature_celsius().await.unwrap();
        let fahrenheit: CentiFahrenheit = driver.temperature_fahrenheit().await.unwrap();

        assert_eq!(celsius.centi_degrees(), centi_celsius);
        assert_eq!(fahrenheit.centi_degrees(), centi_fahrenheit);
        assert!((celsius.to_degrees() - f64::from(centi_celsius) / 100.0).abs() < f64::EPSILON);
        assert!((fahrenheit.to_degrees() - f64::from(centi_fahrenheit) / 100.0).abs() < f64::EPSILON);
        driver.device.interface().bus.done();
    }
}

#[tokio::test]
async fn temperature_conversions_propagate_bus_errors() {
    let transaction = embedded_hal_mock::eh1::i2c::Transaction::write_read(crate::BQ41Z50_ADDR, vec![0x08], vec![0; 2])
        .with_error(ErrorKind::Other);
    let expectations = [transaction.clone(), transaction];
    let mut driver = Bq41z50::new(Mock::new(&expectations));

    assert!(matches!(
        driver.temperature_celsius().await,
        Err(Bq41z50Error::Bus(ErrorKind::Other))
    ));
    assert!(matches!(
        driver.temperature_fahrenheit().await,
        Err(Bq41z50Error::Bus(ErrorKind::Other))
    ));
    driver.device.interface().bus.done();
}

#[tokio::test]
async fn mac_command_no_input_no_output() {
    let expectations = vec![embedded_hal_mock::eh1::i2c::Transaction::write(
        crate::BQ41Z50_ADDR,
        vec![0x44, 0x02, 0x21, 0x00],
    )];

    let mut device = crate::device::Device::new(crate::Interface {
        bus: embedded_hal_mock::eh1::i2c::Mock::new(&expectations),
    });

    device.mac_gauging().dispatch_async().await.unwrap();
    device.interface().bus.done();
}

#[tokio::test]
async fn mac_command_chemical_id() {
    let expectations = vec![
        embedded_hal_mock::eh1::i2c::Transaction::write(crate::BQ41Z50_ADDR, vec![0x44, 0x02, 0x06, 0x00]),
        embedded_hal_mock::eh1::i2c::Transaction::write_read(
            crate::BQ41Z50_ADDR,
            vec![0x44],
            vec![0x04, 0x06, 0x00, 0x00, 0x01],
        ),
    ];

    let mut driver = Bq41z50::new(Mock::new(&expectations));
    let chem_id = driver
        .device
        .mac_chem_id()
        .dispatch_out_async()
        .await
        .unwrap()
        .chem_id();

    assert_eq!(chem_id, 0x0100);
    driver.device.interface().bus.done();
}

#[tokio::test]
async fn security_keys_wire_format() {
    let keys = [0x10, 0x32, 0x54, 0x76, 0x98, 0xBA, 0xDC, 0xFE];
    let expectations = vec![
        embedded_hal_mock::eh1::i2c::Transaction::write(
            crate::BQ41Z50_ADDR,
            vec![0x44, 0x0A, 0x35, 0x00, 0x10, 0x32, 0x54, 0x76, 0x98, 0xBA, 0xDC, 0xFE],
        ),
        embedded_hal_mock::eh1::i2c::Transaction::write(crate::BQ41Z50_ADDR, vec![0x44, 0x02, 0x35, 0x00]),
        embedded_hal_mock::eh1::i2c::Transaction::write_read(
            crate::BQ41Z50_ADDR,
            vec![0x44],
            vec![0x0A, 0x35, 0x00, 0x10, 0x32, 0x54, 0x76, 0x98, 0xBA, 0xDC, 0xFE],
        ),
    ];

    let mut driver = Bq41z50::new(Mock::new(&expectations));
    driver.write_security_keys(&keys).await.unwrap();
    let mut output = [0; 8];
    driver.read_security_keys(&mut output).await.unwrap();

    assert_eq!(output, keys);
    driver.device.interface().bus.done();
}

#[tokio::test]
async fn authentication_key_wire_format() {
    let key = [0xA5; 16];
    let mut write = vec![0x44, 0x12, 0x37, 0x00];
    write.extend_from_slice(&key);
    let mut response = vec![0x12, 0x37, 0x00];
    response.extend_from_slice(&key);
    let expectations = vec![
        embedded_hal_mock::eh1::i2c::Transaction::write(crate::BQ41Z50_ADDR, write),
        embedded_hal_mock::eh1::i2c::Transaction::write(crate::BQ41Z50_ADDR, vec![0x44, 0x02, 0x37, 0x00]),
        embedded_hal_mock::eh1::i2c::Transaction::write_read(crate::BQ41Z50_ADDR, vec![0x44], response),
    ];

    let mut driver = Bq41z50::new(Mock::new(&expectations));
    driver.write_authentication_key(&key).await.unwrap();
    let mut output = [0; 16];
    driver.read_authentication_key(&mut output).await.unwrap();

    assert_eq!(output, key);
    driver.device.interface().bus.done();
}

#[tokio::test]
async fn mfg_info_preserves_the_length_byte() {
    let data: Vec<u8> = (0..32).collect();
    let mut write = vec![0x70, 0x20];
    write.extend_from_slice(&data);
    let mut response = vec![0x20];
    response.extend_from_slice(&data);
    let expectations = vec![
        embedded_hal_mock::eh1::i2c::Transaction::write(crate::BQ41Z50_ADDR, write),
        embedded_hal_mock::eh1::i2c::Transaction::write_read(crate::BQ41Z50_ADDR, vec![0x70], response.clone()),
    ];

    let mut driver = Bq41z50::new(Mock::new(&expectations));
    driver.write_mfg_info(&data).await.unwrap();
    let mut output = [0; 33];
    driver.read_mfg_info(&mut output).await.unwrap();

    assert_eq!(output.as_slice(), response);
    driver.device.interface().bus.done();
}

#[tokio::test]
async fn mfg_info_c_sends_access_keys_before_data() {
    for length in [3u8, 32] {
        let data: Vec<u8> = (0..length).collect();
        let mut write = vec![0x44, length + 2, 0x7B, 0x00];
        write.extend_from_slice(&data);
        let mut response = vec![0x22, 0x7B, 0x00];
        response.extend_from_slice(&data);
        let expectations = vec![
            embedded_hal_mock::eh1::i2c::Transaction::write(crate::BQ41Z50_ADDR, vec![0x44, 0x02, 0x34, 0x12]),
            embedded_hal_mock::eh1::i2c::Transaction::write(crate::BQ41Z50_ADDR, vec![0x44, 0x02, 0x78, 0x56]),
            embedded_hal_mock::eh1::i2c::Transaction::write(crate::BQ41Z50_ADDR, write),
            embedded_hal_mock::eh1::i2c::Transaction::write(crate::BQ41Z50_ADDR, vec![0x44, 0x02, 0x7B, 0x00]),
            embedded_hal_mock::eh1::i2c::Transaction::write_read(crate::BQ41Z50_ADDR, vec![0x44], response),
        ];

        let mut driver = Bq41z50::new(Mock::new(&expectations));
        driver.write_mfg_info_c(0x1234, 0x5678, &data).await.unwrap();
        let mut output = vec![0; usize::from(length)];
        driver.read_mfg_info_c(&mut output).await.unwrap();

        assert_eq!(output, data);
        driver.device.interface().bus.done();
    }
}

#[tokio::test]
async fn charging_voltage_override_wire_format() {
    let config = ChargingVoltageOverride {
        low_temp_chrg_mv: 0x1234,
        std_low_temp_chrg_mv: 0x5678,
        std_hi_temp_chrg_mv: 0x9ABC,
        hi_temp_chrg_mv: 0xDEF0,
        recommended_temp_chrg_mv: 0x1357,
    };
    let expectations = vec![
        embedded_hal_mock::eh1::i2c::Transaction::write(
            crate::BQ41Z50_ADDR,
            vec![
                0x44, 0x0C, 0xB0, 0x00, 0x34, 0x12, 0x78, 0x56, 0xBC, 0x9A, 0xF0, 0xDE, 0x57, 0x13,
            ],
        ),
        embedded_hal_mock::eh1::i2c::Transaction::write(crate::BQ41Z50_ADDR, vec![0x44, 0x02, 0xB0, 0x00]),
        embedded_hal_mock::eh1::i2c::Transaction::write_read(
            crate::BQ41Z50_ADDR,
            vec![0x44],
            vec![
                0x0C, 0xB0, 0x00, 0x34, 0x12, 0x78, 0x56, 0xBC, 0x9A, 0xF0, 0xDE, 0x57, 0x13,
            ],
        ),
    ];

    let mut driver = Bq41z50::new(Mock::new(&expectations));
    driver.write_charging_voltage_override(&config).await.unwrap();
    assert_eq!(driver.read_charging_voltage_override().await.unwrap(), config);
    driver.device.interface().bus.done();
}

#[tokio::test]
async fn seal_and_unseal_wire_format() {
    let expectations = vec![
        embedded_hal_mock::eh1::i2c::Transaction::write(crate::BQ41Z50_ADDR, vec![0x44, 0x02, 0x34, 0x12]),
        embedded_hal_mock::eh1::i2c::Transaction::write(crate::BQ41Z50_ADDR, vec![0x44, 0x02, 0x78, 0x56]),
        embedded_hal_mock::eh1::i2c::Transaction::write(crate::BQ41Z50_ADDR, vec![0x44, 0x02, 0x30, 0x00]),
    ];

    let mut driver = Bq41z50::new(Mock::new(&expectations));
    driver.unseal_fg(0x1234, 0x5678).await.unwrap();
    driver.seal_fg().await.unwrap();
    driver.device.interface().bus.done();
}

#[tokio::test]
async fn mac_read_frames_command() {
    let expectations = vec![
        embedded_hal_mock::eh1::i2c::Transaction::write(crate::BQ41Z50_ADDR, vec![0x44, 0x02, 0x34, 0x12]),
        embedded_hal_mock::eh1::i2c::Transaction::write_read(
            crate::BQ41Z50_ADDR,
            vec![0x44],
            vec![0x04, 0x34, 0x12, 0xA5, 0x5A],
        ),
    ];

    let mut interface = Interface::new(Mock::new(&expectations));
    let mut output = [0; 2];
    interface.mac_read(0x1234, &mut output).await.unwrap();

    assert_eq!(output, [0xA5, 0x5A]);
    interface.bus.done();
}

#[tokio::test]
async fn mac_read_returns_command_write_error_without_retrying() {
    let expectations = vec![
        embedded_hal_mock::eh1::i2c::Transaction::write(crate::BQ41Z50_ADDR, vec![0x44, 0x02, 0x35, 0x00])
            .with_error(ErrorKind::Other),
    ];

    let mut driver = Bq41z50::new(Mock::new(&expectations));
    let mut output = [0xCC; 8];
    assert!(matches!(
        driver.read_security_keys(&mut output).await,
        Err(Bq41z50Error::Bus(ErrorKind::Other))
    ));
    assert_eq!(output, [0xCC; 8]);
    driver.device.interface().bus.done();
}

#[tokio::test]
async fn mac_read_returns_response_error_without_retrying() {
    let expectations = vec![
        embedded_hal_mock::eh1::i2c::Transaction::write(crate::BQ41Z50_ADDR, vec![0x44, 0x02, 0x35, 0x00]),
        embedded_hal_mock::eh1::i2c::Transaction::write_read(crate::BQ41Z50_ADDR, vec![0x44], vec![0; 11])
            .with_error(ErrorKind::Other),
    ];

    let mut driver = Bq41z50::new(Mock::new(&expectations));
    let mut output = [0xCC; 8];
    assert!(matches!(
        driver.read_security_keys(&mut output).await,
        Err(Bq41z50Error::Bus(ErrorKind::Other))
    ));
    assert_eq!(output, [0xCC; 8]);
    driver.device.interface().bus.done();
}

#[tokio::test]
async fn access_key_stops_after_the_first_error() {
    let expectations = vec![
        embedded_hal_mock::eh1::i2c::Transaction::write(crate::BQ41Z50_ADDR, vec![0x44, 0x02, 0x34, 0x12])
            .with_error(ErrorKind::Other),
    ];

    let mut driver = Bq41z50::new(Mock::new(&expectations));
    assert!(matches!(
        driver.write_mfg_info_c(0x1234, 0x5678, &[0xA5]).await,
        Err(Bq41z50Error::Bus(ErrorKind::Other))
    ));
    driver.device.interface().bus.done();
}

#[tokio::test]
async fn oversized_data_is_rejected_before_bus_access() {
    let mut driver = Bq41z50::new(Mock::new(&[]));
    assert!(matches!(
        driver.write_mfg_info(&[0; 33]).await,
        Err(Bq41z50Error::DataTooLarge)
    ));
    assert!(matches!(
        driver.read_mfg_info(&mut [0; 34]).await,
        Err(Bq41z50Error::DataTooLarge)
    ));
    assert!(matches!(
        driver.write_mfg_info_c(0x1234, 0x5678, &[0; 33]).await,
        Err(Bq41z50Error::DataTooLarge)
    ));
    assert!(matches!(
        driver.read_mfg_info_c(&mut [0; 33]).await,
        Err(Bq41z50Error::DataTooLarge)
    ));
    assert!(matches!(
        driver.device.interface().mac_read(0x007B, &mut [0; 33]).await,
        Err(Bq41z50Error::DataTooLarge)
    ));
    assert_eq!(
        smart_battery::Error::kind(&Bq41z50Error::<ErrorKind>::DataTooLarge),
        smart_battery::ErrorKind::Other
    );
    driver.device.interface().bus.done();
}

/// Records every delay so tests can check the wait between a MAC command and its read.
#[derive(Default)]
struct RecordingDelay {
    total_ns: u64,
}

impl embedded_hal_async::delay::DelayNs for RecordingDelay {
    async fn delay_ns(&mut self, ns: u32) {
        self.total_ns += u64::from(ns);
    }
}

#[tokio::test]
async fn block_register_read_of_safety_alert_is_not_shifted() {
    let expectations = vec![embedded_hal_mock::eh1::i2c::Transaction::write_read(
        crate::BQ41Z50_ADDR,
        vec![0x50],
        vec![0x04, 0x00, 0x00, 0x00, 0x00],
    )];
    let mut driver = Bq41z50::new(Mock::new(&expectations));

    let alert: [u8; 4] = driver.device.safety_alert().dispatch_out_async().await.unwrap().into();

    assert_eq!(alert, [0; 4]);
    driver.device.interface().bus.done();
}

#[tokio::test]
async fn full_32_byte_block_register_reads_33_bytes() {
    // GaugeStatus3() is the largest block: count byte 0x20, then 32 data bytes.
    let mut response = vec![0x20];
    response.extend(1..=32u8);
    let expectations = vec![embedded_hal_mock::eh1::i2c::Transaction::write_read(
        crate::BQ41Z50_ADDR,
        vec![0x75],
        response,
    )];
    let mut driver = Bq41z50::new(Mock::new(&expectations));

    let status = driver.device.gauge_status_3().dispatch_out_async().await.unwrap();
    let raw: [u8; 32] = status.into();

    assert_eq!(status.qmax_0(), 0x0201);
    assert_eq!(raw[31], 32);
    driver.device.interface().bus.done();
}

#[tokio::test]
async fn ddsl_sizes_match_the_gauge() {
    let mut gauging_status = vec![0x06];
    gauging_status.extend_from_slice(&[0x01, 0x00, 0x00, 0x01, 0xAA, 0xBB]);
    let expectations = vec![
        embedded_hal_mock::eh1::i2c::Transaction::write_read(crate::BQ41Z50_ADDR, vec![0x56], gauging_status),
        embedded_hal_mock::eh1::i2c::Transaction::write(crate::BQ41Z50_ADDR, vec![0x44, 0x02, 0x03, 0x00]),
        embedded_hal_mock::eh1::i2c::Transaction::write_read(
            crate::BQ41Z50_ADDR,
            vec![0x44],
            vec![0x04, 0x03, 0x00, 0x34, 0x12],
        ),
    ];
    let mut driver = Bq41z50::new(Mock::new(&expectations));

    let status = driver.device.gauging_status().dispatch_out_async().await.unwrap();
    assert!(status.fd());
    assert!(status.perf_mode());
    let hw_version = driver
        .device
        .mac_hardware_version()
        .dispatch_out_async()
        .await
        .unwrap()
        .hw_vers();
    assert_eq!(hw_version, 0x1234);
    driver.device.interface().bus.done();
}

#[tokio::test]
async fn firmware_version_fields_are_big_endian() {
    let expectations = vec![
        embedded_hal_mock::eh1::i2c::Transaction::write(crate::BQ41Z50_ADDR, vec![0x44, 0x02, 0x02, 0x00]),
        embedded_hal_mock::eh1::i2c::Transaction::write_read(
            crate::BQ41Z50_ADDR,
            vec![0x44],
            vec![
                0x0D, 0x02, 0x00, // count, echoed command
                0x41, 0x50, // device number
                0x01, 0x02, // version 1.02
                0x00, 0x23, // build 35
                0x01, // firmware type
                0x05, 0x00, // impedance track version
                0x00, 0x00, // reserved
            ],
        ),
    ];
    let mut driver = Bq41z50::new(Mock::new(&expectations));

    let version = driver.device.mac_firmware_version().dispatch_out_async().await.unwrap();

    assert_eq!(version.device_number(), 0x4150);
    assert_eq!(version.version(), 0x0102);
    assert_eq!(version.build_number(), 35);
    assert_eq!(version.firmware_type(), 0x01);
    assert_eq!(version.impedence_track_vers(), 0x0500);
    driver.device.interface().bus.done();
}

#[tokio::test]
async fn signature_macs_wait_before_reading() {
    let expectations = vec![
        embedded_hal_mock::eh1::i2c::Transaction::write(crate::BQ41Z50_ADDR, vec![0x44, 0x02, 0x04, 0x00]),
        embedded_hal_mock::eh1::i2c::Transaction::write_read(
            crate::BQ41Z50_ADDR,
            vec![0x44],
            vec![0x04, 0x04, 0x00, 0xCD, 0xAB],
        ),
        embedded_hal_mock::eh1::i2c::Transaction::write(crate::BQ41Z50_ADDR, vec![0x44, 0x02, 0x05, 0x00]),
        embedded_hal_mock::eh1::i2c::Transaction::write_read(
            crate::BQ41Z50_ADDR,
            vec![0x44],
            vec![0x04, 0x05, 0x00, 0x34, 0x92],
        ),
        embedded_hal_mock::eh1::i2c::Transaction::write(crate::BQ41Z50_ADDR, vec![0x44, 0x02, 0x09, 0x00]),
        embedded_hal_mock::eh1::i2c::Transaction::write_read(
            crate::BQ41Z50_ADDR,
            vec![0x44],
            vec![0x04, 0x09, 0x00, 0x78, 0x56],
        ),
    ];
    let mut driver = Bq41z50::new(Mock::new(&expectations));
    let mut delay = RecordingDelay::default();

    let insn = driver.read_instruction_flash_signature(&mut delay).await.unwrap();
    assert_eq!(insn.insn_flsh_sig(), 0xABCD);
    assert_eq!(delay.total_ns, 500_000_000);

    let static_df = driver.read_static_df_signature(&mut delay).await.unwrap();
    assert_eq!(static_df.static_df_sig(), 0x1234);
    assert!(static_df.sig_mismatch());
    assert_eq!(delay.total_ns, 1_500_000_000);

    let all_df = driver.read_all_df_signature(&mut delay).await.unwrap();
    assert_eq!(all_df.static_chem_df_sig(), 0x5678);
    assert_eq!(delay.total_ns, 2_500_000_000);
    driver.device.interface().bus.done();
}

#[tokio::test]
async fn battery_status_returns_the_error_code_instead_of_failing() {
    use embedded_batteries_async::smart_battery::SmartBattery;

    // INIT set, EC = Busy from the previous command.
    let expectations = vec![embedded_hal_mock::eh1::i2c::Transaction::write_read(
        crate::BQ41Z50_ADDR,
        vec![0x16],
        vec![0x81, 0x00],
    )];
    let mut driver = Bq41z50::new(Mock::new(&expectations));

    let status = driver.battery_status().await.unwrap();

    assert_eq!(status.error_code(), smart_battery::ErrorCode::Busy);
    assert!(status.initialized());
    driver.device.interface().bus.done();
}

fn sbs_string_block(text: &[u8]) -> Vec<u8> {
    let mut block = vec![u8::try_from(text.len()).unwrap()];
    block.extend_from_slice(text);
    block.resize(33, 0xFF);
    block
}

#[tokio::test]
async fn sbs_strings_drop_the_length_byte_and_are_nul_terminated() {
    use embedded_batteries_async::smart_battery::SmartBattery;

    let expectations = vec![
        embedded_hal_mock::eh1::i2c::Transaction::write_read(
            crate::BQ41Z50_ADDR,
            vec![0x20],
            sbs_string_block(b"Texas Inst."),
        ),
        embedded_hal_mock::eh1::i2c::Transaction::write_read(
            crate::BQ41Z50_ADDR,
            vec![0x21],
            sbs_string_block(b"bq41z50"),
        ),
        embedded_hal_mock::eh1::i2c::Transaction::write_read(
            crate::BQ41Z50_ADDR,
            vec![0x22],
            sbs_string_block(b"LION"),
        ),
    ];
    let mut driver = Bq41z50::new(Mock::new(&expectations));

    let mut name = [0xEEu8; 16];
    driver.manufacturer_name(&mut name).await.unwrap();
    assert_eq!(&name, b"Texas Inst.\0\0\0\0\0");

    // Truncated to fit, still NUL-terminated.
    let mut name = [0xEEu8; 4];
    driver.device_name(&mut name).await.unwrap();
    assert_eq!(&name, b"bq4\0");

    let mut chemistry = [0xEEu8; 8];
    driver.device_chemistry(&mut chemistry).await.unwrap();
    assert_eq!(&chemistry, b"LION\0\0\0\0");
    driver.device.interface().bus.done();
}
