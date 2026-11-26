//! Offline spell checking through the OS spell checker on Windows (`ISpellCheckerFactory`)
//! and system Hunspell dictionaries on Linux (`/usr/share/hunspell` & `/usr/share/myspell`).
//! Results are cached per paragraph text.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

#[cfg(windows)]
use windows::{
    core::{HSTRING, PWSTR},
    Win32::Globalization::{ISpellChecker, ISpellCheckerFactory, SpellCheckerFactory},
    Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED},
};

pub const MAX_SUGGESTIONS: usize = 6;
const CACHE_LIMIT: usize = 8000;

pub struct Speller {
    #[cfg(windows)]
    checker: Option<ISpellChecker>,
    #[cfg(target_os = "linux")]
    dict: Option<spellbook::Dictionary>,
    #[cfg(target_os = "linux")]
    custom_words: RefCell<HashSet<String>>,
    cache: RefCell<HashMap<String, Vec<(usize, usize)>>>,
    ignored: RefCell<HashSet<String>>,
    pub language: String,
}

impl Default for Speller {
    fn default() -> Self {
        Self::new()
    }
}

impl Speller {
    pub fn new() -> Self {
        #[cfg(windows)]
        {
            // SAFETY: `Self::create` initializes COM on the current thread and queries the
            // inbox Windows `ISpellCheckerFactory` COM server using valid Win32 parameters.
            let (checker, language) = unsafe { Self::create() }.unwrap_or((None, String::new()));
            Self { checker, cache: Default::default(), ignored: Default::default(), language }
        }
        #[cfg(target_os = "linux")]
        {
            let (dict, language) = Self::load_linux_hunspell();
            let custom_words = Self::load_custom_words();
            Self {
                dict,
                custom_words: RefCell::new(custom_words),
                cache: Default::default(),
                ignored: Default::default(),
                language,
            }
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            Self { cache: Default::default(), ignored: Default::default(), language: String::new() }
        }
    }

    #[cfg(target_os = "linux")]
    fn custom_dict_path() -> Option<std::path::PathBuf> {
        dirs::config_dir().map(|d| d.join("Syllabary").join("custom_dict.txt"))
    }

    #[cfg(target_os = "linux")]
    fn load_custom_words() -> HashSet<String> {
        let Some(path) = Self::custom_dict_path() else {
            return HashSet::new();
        };
        std::fs::read_to_string(path)
            .map(|s| {
                s.lines()
                    .map(|l| l.trim().to_lowercase())
                    .filter(|l| !l.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }

    #[cfg(target_os = "linux")]
    fn load_linux_hunspell() -> (Option<spellbook::Dictionary>, String) {
        let mut tags: Vec<String> = Vec::new();
        for env_key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Ok(val) = std::env::var(env_key) {
                let tag = val.split('.').next().unwrap_or("").split('@').next().unwrap_or("");
                if !tag.is_empty() && tag != "C" && tag != "POSIX" && !tags.iter().any(|t| t == tag) {
                    tags.push(tag.to_string());
                }
            }
        }
        for fallback in ["en_US", "en_GB", "en_AU", "en_CA"] {
            if !tags.iter().any(|t| t == fallback) {
                tags.push(fallback.to_string());
            }
        }

        let mut search_dirs: Vec<std::path::PathBuf> = vec![
            std::path::PathBuf::from("/usr/share/hunspell"),
            std::path::PathBuf::from("/usr/share/myspell"),
            std::path::PathBuf::from("/usr/local/share/hunspell"),
        ];
        if let Some(data_dir) = dirs::data_local_dir() {
            search_dirs.push(data_dir.join("hunspell"));
        }

        for tag in &tags {
            for dir in &search_dirs {
                let aff_path = dir.join(format!("{tag}.aff"));
                let dic_path = dir.join(format!("{tag}.dic"));
                if aff_path.exists() && dic_path.exists() {
                    let Ok(aff_bytes) = std::fs::read(&aff_path) else { continue };
                    let Ok(dic_bytes) = std::fs::read(&dic_path) else { continue };
                    let aff = String::from_utf8_lossy(&aff_bytes);
                    let dic = String::from_utf8_lossy(&dic_bytes);
                    if let Ok(dict) = spellbook::Dictionary::new(&aff, &dic) {
                        return (Some(dict), tag.replace('_', "-"));
                    }
                }
            }
        }
        (None, String::new())
    }

    #[cfg(windows)]
    unsafe fn create() -> windows::core::Result<(Option<ISpellChecker>, String)> {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let factory: ISpellCheckerFactory = CoCreateInstance(&SpellCheckerFactory, None, CLSCTX_INPROC_SERVER)?;
        let mut buf = [0u16; 85];
        let n = windows::Win32::Globalization::GetUserDefaultLocaleName(&mut buf);
        let user = if n > 1 { String::from_utf16_lossy(&buf[..(n - 1) as usize]) } else { String::new() };
        let mut tags: Vec<String> = Vec::new();
        if !user.is_empty() {
            tags.push(user);
        }
        for t in ["en-US", "en-GB", "en-AU", "en-CA"] {
            tags.push(t.to_string());
        }
        for tag in tags {
            let h = HSTRING::from(tag.as_str());
            if factory.IsSupported(&h).is_ok_and(|b| b.as_bool()) {
                let c = factory.CreateSpellChecker(&h)?;
                return Ok((Some(c), tag));
            }
        }
        Ok((None, String::new()))
    }

    pub fn available(&self) -> bool {
        #[cfg(windows)]
        {
            self.checker.is_some()
        }
        #[cfg(target_os = "linux")]
        {
            self.dict.is_some()
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            false
        }
    }

    /// Misspelled ranges (char indices) in one paragraph.
    pub fn errors(&self, text: &[char]) -> Vec<(usize, usize)> {
        if text.is_empty() || !self.available() {
            return Vec::new();
        }
        let s: String = text.iter().collect();
        if let Some(v) = self.cache.borrow().get(&s) {
            return self.filter(text, v);
        }
        let v = self.check_raw(&s, text);
        let mut c = self.cache.borrow_mut();
        if c.len() > CACHE_LIMIT {
            c.clear();
        }
        c.insert(s, v.clone());
        drop(c);
        self.filter(text, &v)
    }

    fn filter(&self, text: &[char], v: &[(usize, usize)]) -> Vec<(usize, usize)> {
        let ig = self.ignored.borrow();
        if ig.is_empty() {
            return v.to_vec();
        }
        v.iter().copied().filter(|(a, b)| !ig.contains(&text[*a..*b].iter().collect::<String>().to_lowercase())).collect()
    }

    #[cfg_attr(not(any(windows, target_os = "linux")), expect(unused_variables, reason = "Spellchecker backend is only available on Windows and Linux"))]
    #[cfg_attr(target_os = "linux", expect(unused_variables, reason = "Linux Hunspell backend iterates char slices directly"))]
    fn check_raw(&self, s: &str, text: &[char]) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        #[cfg(windows)]
        // SAFETY: `self.checker` is a valid COM interface pointer on this apartment thread,
        // and `HSTRING` keeps the UTF-16 buffer alive for the duration of `Check`.
        unsafe {
            let Some(chk) = &self.checker else { return out };
            // map UTF-16 offsets to char indices
            let mut u16_to_char = Vec::with_capacity(s.len() + 1);
            for (ci, ch) in text.iter().enumerate() {
                for _ in 0..ch.len_utf16() {
                    u16_to_char.push(ci);
                }
            }
            u16_to_char.push(text.len());
            let Ok(errs) = chk.Check(&HSTRING::from(s)) else { return out };
            loop {
                let mut e = None;
                if errs.Next(&mut e).0 != 0 {
                    break;
                }
                let Some(e) = e else { break };
                let (Ok(st), Ok(len)) = (e.StartIndex(), e.Length()) else { continue };
                let a = *u16_to_char.get(st as usize).unwrap_or(&text.len());
                let b = *u16_to_char.get((st + len) as usize).unwrap_or(&text.len());
                if b > a {
                    out.push((a, b));
                }
            }
        }
        #[cfg(target_os = "linux")]
        {
            let Some(dict) = &self.dict else { return out };
            let custom = self.custom_words.borrow();
            let mut i = 0;
            while i < text.len() {
                if !text[i].is_alphabetic() {
                    i += 1;
                    continue;
                }
                let start = i;
                while i < text.len()
                    && (text[i].is_alphabetic()
                        || ((text[i] == '\'' || text[i] == '\u{2019}')
                            && i + 1 < text.len()
                            && text[i + 1].is_alphabetic()))
                {
                    i += 1;
                }
                let end = i;
                if end > start {
                    let word: String = text[start..end]
                        .iter()
                        .map(|&c| if c == '\u{2019}' { '\'' } else { c })
                        .collect();
                    if !custom.contains(&word.to_lowercase()) && !dict.check(&word) {
                        out.push((start, end));
                    }
                }
            }
        }
        out
    }

    #[cfg_attr(not(any(windows, target_os = "linux")), expect(unused_variables, reason = "Spellchecker backend is only available on Windows and Linux"))]
    pub fn suggest(&self, word: &str) -> Vec<String> {
        let mut out = Vec::new();
        #[cfg(windows)]
        // SAFETY: `list.Next` returns a COM-allocated `PWSTR` which we convert to a Rust `String`
        // and immediately free with `CoTaskMemFree` as required by the `IEnumString` contract.
        unsafe {
            let Some(chk) = &self.checker else { return out };
            let Ok(list) = chk.Suggest(&HSTRING::from(word)) else { return out };
            while out.len() < MAX_SUGGESTIONS {
                let mut buf = [PWSTR::null()];
                let mut got = 0u32;
                if list.Next(&mut buf, Some(&mut got)).0 != 0 || got == 0 {
                    break;
                }
                if let Ok(s) = buf[0].to_string() {
                    out.push(s);
                }
                CoTaskMemFree(Some(buf[0].0 as *const _));
            }
        }
        #[cfg(target_os = "linux")]
        {
            if let Some(dict) = &self.dict {
                dict.suggest(word, &mut out);
                out.truncate(MAX_SUGGESTIONS);
            }
        }
        out
    }

    /// Add to the computer's own custom dictionary (Windows registry/file or `~/.config/Syllabary/custom_dict.txt` on Linux).
    #[cfg_attr(not(any(windows, target_os = "linux")), expect(unused_variables, reason = "Spellchecker backend is only available on Windows and Linux"))]
    pub fn add(&self, word: &str) {
        #[cfg(windows)]
        // SAFETY: `chk.Add` takes a valid `HSTRING` reference on the initialized COM apartment.
        unsafe {
            if let Some(chk) = &self.checker {
                let _ = chk.Add(&HSTRING::from(word));
            }
        }
        #[cfg(target_os = "linux")]
        {
            let clean = word.trim().to_lowercase();
            if !clean.is_empty() && self.custom_words.borrow_mut().insert(clean) {
                if let Some(path) = Self::custom_dict_path() {
                    if let Some(parent) = path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    let mut all: Vec<String> = self.custom_words.borrow().iter().cloned().collect();
                    all.sort();
                    let _ = std::fs::write(path, all.join("\n") + "\n");
                }
            }
        }
        self.cache.borrow_mut().clear();
    }

    /// Ignore for the rest of this session.
    pub fn ignore(&self, word: &str) {
        self.ignored.borrow_mut().insert(word.to_lowercase());
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn flags_misspelling_when_dictionary_present() {
        let sp = super::Speller::new();
        if !sp.available() {
            return; // no English dictionary on this computer
        }
        let text: Vec<char> = "This is a smple sentence".chars().collect();
        let e = sp.errors(&text);
        assert_eq!(e, vec![(10, 15)]);
        assert!(sp.suggest("smple").iter().any(|s| s == "simple" || s == "sample"));
        sp.ignore("smple");
        assert!(sp.errors(&text).is_empty());
    }
}
