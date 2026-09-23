/// Serialize a value to RFC 8785 canonical JSON bytes (spec section 28).
/// This is what gets hashed/signed — never hash arbitrary incoming JSON text
/// directly, since whitespace/field order must not change identity.
pub fn canonical_json_bytes<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, serde_json::Error> {
    serde_jcs::to_vec(value)
}
