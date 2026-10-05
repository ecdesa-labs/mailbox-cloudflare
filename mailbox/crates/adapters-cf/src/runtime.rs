use mailbox_application::{Clock, IdGenerator};
use mailbox_domain::{MessageId, UnixMs};
use wasm_bindgen::JsCast;

pub struct WorkerClock;

impl Clock for WorkerClock {
    fn now(&self) -> UnixMs {
        // Antes de 1970 só com relógio quebrado; nesse caso, 0.
        UnixMs::new(worker::Date::now().as_millis() as i64)
            .unwrap_or_else(|_| UnixMs::new(0).unwrap())
    }
}

/// ULID com os 80 bits aleatórios do `crypto.getRandomValues` do runtime.
pub struct CryptoIds;

impl IdGenerator for CryptoIds {
    fn message_id(&self, at: UnixMs) -> MessageId {
        let mut buf = [0u8; 16];
        let crypto: web_sys::Crypto = js_sys::Reflect::get(&js_sys::global(), &"crypto".into())
            .expect("globalThis.crypto existe no Workers")
            .unchecked_into();
        crypto
            .get_random_values_with_u8_array(&mut buf[6..])
            .expect("getRandomValues com 10 bytes não falha");
        MessageId::from_parts(at, u128::from_be_bytes(buf))
    }
}
