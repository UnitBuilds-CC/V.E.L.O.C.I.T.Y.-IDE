use velocity_browser::engine::{GeolocationProvider, PaymentRequestEngine, WebAudioEngine};
use velocity_browser::net::WebBluetoothTransport;

#[test]
fn test_payment_and_geolocation_engines() {
    let mut pay = PaymentRequestEngine::new("Test Merchant");
    pay.add_item("Pro Subscription", 29.99, "USD");
    assert!(pay.show().is_ok());

    let geo = GeolocationProvider::mock_sf();
    let pos = geo.get_current_position();
    assert_eq!(pos.latitude, 37.7749);
}

#[test]
fn test_bluetooth_and_audio_engines() {
    let mut bt = WebBluetoothTransport::new();
    let dev = bt.request_device("HeartRateMonitor");
    assert!(dev.is_some());

    let mut audio = WebAudioEngine::new(44100);
    let osc_id = audio.create_oscillator(440.0);
    assert_eq!(osc_id, 1);
}
