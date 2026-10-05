//! Casos de uso e portas (traits). Não pode importar `worker`.
//! Portas: MessageRepository, BlobStore, IngestQueue, Mailer, Clock, IdGenerator.

#[cfg(test)]
mod tests {
    #[test]
    fn smoke() {
        assert_eq!(2 + 2, 4);
    }
}
