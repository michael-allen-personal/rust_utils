use error_set::error_set;

error_set! {
    ParsingError := {
        #[display("Invalid value '{value}' for type '{to_type}', expected one of: {expected}")]
        InvalidStringEnumValue {
            value: String,
            to_type: &'static str,
            expected: &'static str,
        }
    }
}
