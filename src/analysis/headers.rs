use std::path::Path;

use super::Worker;
use crate::Result;
use crate::languages::LanguageChoice;
use crate::languages::c_family::has_cpp_syntax;

impl Worker {
    /// C and C++ share the `.h` extension. Use C++ for a header that the C
    /// grammar cannot parse when the C++ grammar parses it or still finds C++
    /// syntax around its errors, so C headers that use C++ keywords as names
    /// or macros that neither grammar parses stay C.
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
        if !self
            .parse_file(&choice, source, None)?
            .root_node()
            .has_error()
        {
            return Ok(choice);
        }
        let cpp_tree = self.parse_file(&cpp, source, None)?;
        if cpp_tree.root_node().has_error() && !has_cpp_syntax(cpp_tree.root_node()) {
            return Ok(choice);
        }
        Ok(cpp)
    }
}
