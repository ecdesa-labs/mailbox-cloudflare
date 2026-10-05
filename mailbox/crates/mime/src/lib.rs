//! bytes -> ParsedMessage, sanitização, reescrita de cid e imagens remotas.
//! Puro. Conteúdo real na Fase 2.

#[cfg(test)]
mod tests {
    use mail_parser::MessageParser;

    #[test]
    fn parser_roda_nativo() {
        let raw = b"Message-ID: <a@b>\r\nSubject: oi\r\n\r\ncorpo";
        let msg = MessageParser::default().parse(&raw[..]).unwrap();
        assert_eq!(msg.subject(), Some("oi"));
    }
}
