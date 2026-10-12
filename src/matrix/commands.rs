#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Reset,
    Store(String),
    Chat(String),
}

pub fn parse(message: &str) -> Command {
    let trimmed = message.trim_end();
    let Some(rest) = trimmed
        .strip_prefix("!mynd ")
        .or_else(|| if trimmed == "!mynd" { Some("") } else { None })
    else {
        return Command::Chat(message.to_string());
    };
    let rest = rest.trim();
    if rest == "reset" {
        return Command::Reset;
    }
    if let Some(text) = rest.strip_prefix("store ") {
        let text = text.trim();
        if !text.is_empty() {
            return Command::Store(text.to_string());
        }
    }
    Command::Chat(message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_message_is_chat() {
        assert_eq!(
            parse("what's my postgres preference?"),
            Command::Chat("what's my postgres preference?".to_string())
        );
    }

    #[test]
    fn hm_reset_is_recognized() {
        assert_eq!(parse("!mynd reset"), Command::Reset);
    }

    #[test]
    fn hm_reset_trims_trailing_whitespace() {
        assert_eq!(parse("!mynd reset   "), Command::Reset);
    }

    #[test]
    fn hm_store_captures_the_rest_of_the_message() {
        assert_eq!(
            parse("!mynd store use tabs not spaces in go"),
            Command::Store("use tabs not spaces in go".to_string())
        );
    }

    #[test]
    fn hm_store_with_no_text_is_chat_not_an_empty_store() {
        // Empty payload is ambiguous/useless as a memory — treat the whole
        // thing as an ordinary chat message instead of storing "".
        assert_eq!(parse("!mynd store"), Command::Chat("!mynd store".to_string()));
        assert_eq!(
            parse("!mynd store   "),
            Command::Chat("!mynd store   ".to_string())
        );
    }

    #[test]
    fn unknown_mynd_subcommand_falls_back_to_chat() {
        assert_eq!(
            parse("!mynd frobnicate"),
            Command::Chat("!mynd frobnicate".to_string())
        );
    }

    #[test]
    fn hm_prefix_without_leading_bang_is_chat() {
        assert_eq!(parse("mynd reset"), Command::Chat("mynd reset".to_string()));
    }
}
