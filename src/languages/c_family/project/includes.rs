use super::*;

impl Inventory<'_> {
    pub(super) fn include(
        &mut self,
        from: &Path,
        name: &str,
        quoted: bool,
        settings: &Settings,
    ) -> Option<PathBuf> {
        let local = from.parent().filter(|_| quoted);
        local
            .into_iter()
            .chain(
                settings
                    .quote
                    .iter()
                    .map(PathBuf::as_path)
                    .filter(|_| quoted),
            )
            .chain(
                settings
                    .include
                    .iter()
                    .chain(&settings.system)
                    .map(PathBuf::as_path),
            )
            .map(|dir| dir.join(name))
            .find_map(|path| {
                let path = crate::input::path_identity(&path).ok()?;
                if !self.sources.contains_key(&path) {
                    let bytes = self.snapshot.read(&path).ok()??;
                    self.sources
                        .insert(path.clone(), Arc::new(Source::new(&bytes)));
                }
                Some(path)
            })
    }
}

pub(super) fn spelling<'a>(value: &'a str, macros: &'a Environment) -> Option<(&'a str, bool)> {
    let value = match macros.macros.get(value) {
        Some(Macro::Defined(value)) => value.trim(),
        _ => value,
    };
    if let Some(value) = value.strip_prefix('"') {
        return Some((value.split_once('"')?.0, true));
    }
    Some((value.strip_prefix('<')?.split_once('>')?.0, false))
}

/// The standard library is an external boundary when its headers are absent.
pub(super) fn standard(name: &str) -> bool {
    const NAMES: &str = "assert.h complex.h ctype.h errno.h fenv.h float.h inttypes.h iso646.h limits.h locale.h math.h setjmp.h signal.h stdalign.h stdarg.h stdatomic.h stdbit.h stdbool.h stdckdint.h stddef.h stdint.h stdio.h stdlib.h stdnoreturn.h string.h tgmath.h threads.h time.h uchar.h wchar.h wctype.h algorithm any array atomic barrier bit bitset cassert cctype cerrno cfenv cfloat charconv chrono cinttypes ciso646 climits clocale cmath codecvt compare complex concepts condition_variable coroutine csetjmp csignal cstdarg cstdbool cstddef cstdint cstdio cstdlib cstring ctgmath ctime cuchar cwchar cwctype deque exception execution expected filesystem flat_map flat_set format forward_list fstream functional future generator initializer_list iomanip ios iosfwd iostream istream iterator latch limits list locale map mdspan memory memory_resource mutex new numbers numeric optional ostream print queue random ranges ratio regex scoped_allocator semaphore set shared_mutex source_location span spanstream sstream stack stacktrace stdexcept stdfloat stop_token streambuf string string_view strstream syncstream system_error thread tuple type_traits typeindex typeinfo unordered_map unordered_set utility valarray variant vector version";
    NAMES.split_ascii_whitespace().any(|header| header == name)
}
