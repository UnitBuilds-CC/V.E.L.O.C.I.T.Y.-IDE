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
        // SAFETY: `Library::new` loads a dynamic library. The path comes from the caller.
        // The library is kept alive via `lib` for the lifetime of the returned DynamicParser.
        let lib = unsafe { Library::new(lib_path.as_ref())? };
        // SAFETY: The symbol is looked up in the loaded library and cast to a function pointer.
        // The function is called immediately to obtain the Language struct. The transmute
        // converts the raw pointer to a Language value, which is valid because the symbol
        // is guaranteed to return a properly initialized Language by the plugin contract.
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
