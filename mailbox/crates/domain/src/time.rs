use std::fmt;

use crate::DomainError;

/// Instante em milissegundos Unix (UTC). Nunca lê o relógio: quem precisa
/// de "agora" usa a porta `Clock`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UnixMs(i64);

impl UnixMs {
    /// Recusa negativos: nada neste sistema acontece antes de 1970.
    pub fn new(ms: i64) -> Result<Self, DomainError> {
        if ms < 0 {
            return Err(DomainError::InvalidTimestamp(ms));
        }
        Ok(Self(ms))
    }

    pub fn as_i64(self) -> i64 {
        self.0
    }

    /// Ano e mês (1..=12) em UTC. Usado no layout de chaves do R2.
    pub fn year_month(self) -> (i64, u32) {
        let days = self.0.div_euclid(86_400_000);
        let (y, m, _) = civil_from_days(days);
        (y, m)
    }
}

impl fmt::Display for UnixMs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Dias desde 1970-01-01 → (ano, mês, dia). Algoritmo de Howard Hinnant.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recusa_negativo() {
        assert!(UnixMs::new(-1).is_err());
        assert!(UnixMs::new(0).is_ok());
    }

    #[test]
    fn ano_mes() {
        let cases = [
            (0, (1970, 1)),
            (1_791_158_400_000, (2026, 10)), // 2026-10-05T00:00Z
            (1_790_812_799_999, (2026, 9)),  // 2026-09-30T23:59:59.999Z
            (1_790_812_800_000, (2026, 10)), // 2026-10-01T00:00Z
            (951_782_400_000, (2000, 2)),    // 2000-02-29
            (951_868_800_000, (2000, 3)),    // 2000-03-01
            (1_704_067_199_999, (2023, 12)),
            (1_704_067_200_000, (2024, 1)),
        ];
        for (ms, esperado) in cases {
            assert_eq!(UnixMs::new(ms).unwrap().year_month(), esperado, "{ms}");
        }
    }

    #[test]
    fn civil_dias_conhecidos() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(20_731), (2026, 10, 5));
    }
}
