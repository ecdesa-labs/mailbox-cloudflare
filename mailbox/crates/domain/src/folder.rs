use std::fmt;
use std::str::FromStr;

use crate::DomainError;

/// Pasta de uma mensagem. A string é a mesma da coluna `messages.folder`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Folder {
    #[default]
    Inbox,
    Archive,
    Trash,
    Spam,
    Sent,
}

impl Folder {
    pub const ALL: [Folder; 5] = [
        Folder::Inbox,
        Folder::Archive,
        Folder::Trash,
        Folder::Spam,
        Folder::Sent,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Folder::Inbox => "inbox",
            Folder::Archive => "archive",
            Folder::Trash => "trash",
            Folder::Spam => "spam",
            Folder::Sent => "sent",
        }
    }
}

impl fmt::Display for Folder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Folder {
    type Err = DomainError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Folder::ALL
            .into_iter()
            .find(|f| f.as_str() == s)
            .ok_or_else(|| DomainError::InvalidFolder(s.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ida_e_volta() {
        for f in Folder::ALL {
            assert_eq!(f.as_str().parse::<Folder>().unwrap(), f);
        }
        assert_eq!(Folder::default(), Folder::Inbox);
    }

    #[test]
    fn recusa_desconhecida_e_maiuscula() {
        assert!("drafts".parse::<Folder>().is_err());
        assert!("Inbox".parse::<Folder>().is_err());
    }
}
