use std::fmt;

/// Como a mensagem foi processada (`messages.processing`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Processing {
    /// Corpo, anexos, HTML e busca.
    Full,
    /// Modo leve (docs seção 6.2, passo 0): só headers. O `.eml` fica para
    /// download. Usado quando o email é grande demais para os 10 ms de CPU
    /// do plano Free.
    Partial,
}

impl Processing {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Partial => "partial",
        }
    }

    /// Acima de `full_max_bytes`, modo leve.
    pub fn for_size(size: u64, full_max_bytes: u64) -> Self {
        if size > full_max_bytes {
            Self::Partial
        } else {
            Self::Full
        }
    }
}

impl fmt::Display for Processing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limiar() {
        assert_eq!(Processing::for_size(100, 100), Processing::Full);
        assert_eq!(Processing::for_size(101, 100), Processing::Partial);
        assert_eq!(Processing::Partial.as_str(), "partial");
    }
}
