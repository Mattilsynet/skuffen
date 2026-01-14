#[cfg(test)]
mod tests {
    use crate::model::sak::{Ordningsverdi, Saksnummer, Sakstittel};
    use std::str::FromStr;

    #[test]
    fn sakstittel_validation() {
        assert!(Sakstittel::from_str("Valid title").is_ok());
        assert!(Sakstittel::from_str("   ").is_err());
        assert!(Sakstittel::from_str("").is_err());
        assert!(Sakstittel::from_str(&"a".repeat(257)).is_err());
    }

    #[test]
    fn ordningsverdi_validation() {
        assert!(Ordningsverdi::new("2020".to_string()).is_ok());
        assert!(Ordningsverdi::new("".to_string()).is_err());
        assert!(Ordningsverdi::new("abc".to_string()).is_err()); // digits or -
        assert!(Ordningsverdi::new("123-456".to_string()).is_ok());
        assert!(Ordningsverdi::new("123-456-789".to_string()).is_err()); // max 1 hyphen
    }

    #[test]
    fn saksnummer_validation() {
        assert!(Saksnummer::new("2021/12345").is_ok());
        assert!(Saksnummer::new("2021").is_err()); // missing seq
        assert!(Saksnummer::new("abcd/12345").is_err()); // year not u16
        assert!(Saksnummer::new("999/12345").is_err()); // year < 1000
        assert!(Saksnummer::new("10000/12345").is_err()); // year > 9999
        assert!(Saksnummer::new("2021/").is_err()); // empty seq
    }

    #[test]
    fn saksnummer_accessors() {
        let sn = Saksnummer::new("2021/12345").unwrap();
        assert_eq!(sn.year(), 2021);
        assert_eq!(sn.sequence(), "12345");
        assert_eq!(sn.as_str(), "2021/12345");
    }
}
