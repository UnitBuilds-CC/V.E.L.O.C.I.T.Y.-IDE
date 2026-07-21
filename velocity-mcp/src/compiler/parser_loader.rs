use libloading::Library;
use std::error::Error;
use std::path::Path;
use tree_sitter::Language;

pub struct DynamicParser {
    _lib: Library,
    language: Language,
}

impl DynamicParser {
    pub fn load<P: AsRef<Path>>(lib_path: P, symbol_name: &str) -> Result<Self, Box<dyn Error>> {
        let lib = unsafe { Library::new(lib_path.as_ref())? };
        let language = unsafe {
            let symbol: libloading::Symbol<unsafe extern "C" fn() -> *const std::ffi::c_void> =
                lib.get(symbol_name.as_bytes())?;
            let raw_lang = symbol();
            std::mem::transmute::<*const std::ffi::c_void, Language>(raw_lang)
        };
        Ok(Self {
            _lib: lib,
            language,
        })
    }

    pub fn language(&self) -> Language {
        self.language
    }
}
