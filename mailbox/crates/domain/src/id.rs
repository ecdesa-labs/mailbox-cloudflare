//! IDs tipados por entidade, todos ULID por baixo. Um `MessageId` nunca é
//! aceito onde se espera um `ThreadId`.

use std::fmt;
use std::str::FromStr;

use ulid::Ulid;

use crate::{DomainError, UnixMs};

macro_rules! entity_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(Ulid);

        impl $name {
            /// Monta a partir de tempo + 80 bits aleatórios. Quem chama é a
            /// porta `IdGenerator`; os bits acima de 80 em `random` são ignorados.
            pub fn from_parts(at: UnixMs, random: u128) -> Self {
                Self(Ulid::from_parts(at.as_i64() as u64, random))
            }

            /// Lê a forma canônica de 26 caracteres (Crockford base32).
            /// Recusa o que estoura 128 bits (primeiro caractere > `7`): o
            /// crate `ulid` descarta esses bits em silêncio, e `81M…` viraria
            /// o mesmo ID que `01M…`.
            pub fn parse(s: &str) -> Result<Self, DomainError> {
                let err = || DomainError::InvalidId(s.to_owned());
                let ulid = Ulid::from_string(s).map_err(|_| err())?;
                if !ulid.to_string().eq_ignore_ascii_case(s) {
                    return Err(err());
                }
                Ok(Self(ulid))
            }

            pub fn timestamp(&self) -> UnixMs {
                // 48 bits de ms cabem com folga em i64 positivo.
                UnixMs::new(self.0.timestamp_ms() as i64).expect("48 bits")
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl FromStr for $name {
            type Err = DomainError;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Self::parse(s)
            }
        }
    };
}

entity_id!(DomainId);
entity_id!(IdentityId);
entity_id!(ThreadId);
entity_id!(
    /// Também é o nome do `.eml` no R2.
    MessageId
);
entity_id!(AttachmentId);
entity_id!(RuleId);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ida_e_volta() {
        let at = UnixMs::new(1_791_158_400_000).unwrap();
        let id = MessageId::from_parts(at, 1);
        assert_eq!(id.to_string(), "01M44NRD000000000000000001");
        assert_eq!(MessageId::parse(&id.to_string()).unwrap(), id);
        assert_eq!(id.timestamp(), at);
    }

    #[test]
    fn aceita_minusculas_e_formata_maiusculas() {
        let id: ThreadId = "01m44nrd000000000000000001".parse().unwrap();
        assert_eq!(id.to_string(), "01M44NRD000000000000000001");
    }

    #[test]
    fn recusa_invalidos() {
        for s in [
            "",
            "01M44NRD00000000000000000",   // 25
            "01M44NRD0000000000000000011", // 27
            "01M44NRD00000000000000000U",  // U fora do alfabeto
            "81M44NRD000000000000000001",  // estoura 128 bits
        ] {
            assert!(MessageId::parse(s).is_err(), "{s}");
        }
    }

    #[test]
    fn ordena_por_tempo() {
        let a = MessageId::from_parts(UnixMs::new(1).unwrap(), u128::MAX);
        let b = MessageId::from_parts(UnixMs::new(2).unwrap(), 0);
        assert!(a < b);
        assert!(a.to_string() < b.to_string());
    }
}
