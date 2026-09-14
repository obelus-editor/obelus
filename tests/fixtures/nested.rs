//! Something inside something, so that a fold can hold a fold.
pub struct Thing {
    pub name: String,
}

impl Thing {
    pub fn shout(&self) -> String {
        if self.name.is_empty() {
            return String::new();
        }
        self.name.to_uppercase()
    }
}
