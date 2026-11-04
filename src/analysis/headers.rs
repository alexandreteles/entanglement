use std::path::Path;

use super::Worker;
use crate::Result;
use crate::languages::LanguageChoice;

impl Worker {
    /// C and C++ share the `.h` extension. Use C++ for a header that only the
    /// C++ grammar parses, so C headers that use C++ keywords as names stay C.
    pub(super) fn header_choice(
        &mut self,
        path: &Path,
        source: &[u8],
        choice: LanguageChoice,
    ) -> Result<LanguageChoice> {
        if choice.id != "source.c" || path.extension().is_none_or(|extension| extension != "h") {
            return Ok(choice);
        }
        let Some(cpp) = self.registry.select_injection("cpp")? else {
            return Ok(choice);
        };
        let parses = |worker: &mut Self, language: &LanguageChoice| -> Result<bool> {
            Ok(!worker
                .parse_tree(language, source, &[], None)?
                .root_node()
                .has_error())
        };
        if parses(self, &choice)? || !parses(self, &cpp)? {
            return Ok(choice);
        }
        Ok(cpp)
    }
}
