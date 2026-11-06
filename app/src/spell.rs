//! Offline spell checking through the Windows spell checker that ships with the
//! operating system (no internet). Results are cached per paragraph text.

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
    cache: RefCell<HashMap<String, Vec<(usize, usize)>>>,
    ignored: RefCell<HashSet<String>>,
    pub language: String,
}

impl Speller {
    pub fn new() -> Self {
        #[cfg(windows)]
        {
            let (checker, language) = unsafe { Self::create() }.unwrap_or((None, String::new()));
            return Self { checker, cache: Default::default(), ignored: Default::default(), language };
        }
        #[cfg(not(windows))]
        Self { cache: Default::default(), ignored: Default::default(), language: String::new() }
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
            if factory.IsSupported(&h).map(|b| b.as_bool()).unwrap_or(false) {
                let c = factory.CreateSpellChecker(&h)?;
                return Ok((Some(c), tag));
            }
        }
        Ok((None, String::new()))
    }

    pub fn available(&self) -> bool {
        #[cfg(windows)]
        {
            return self.checker.is_some();
        }
        #[allow(unreachable_code)]
        false
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

    #[allow(unused_variables)]
    fn check_raw(&self, s: &str, text: &[char]) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        #[cfg(windows)]
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
        out
    }

    #[allow(unused_variables)]
    pub fn suggest(&self, word: &str) -> Vec<String> {
        let mut out = Vec::new();
        #[cfg(windows)]
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
        out
    }

    /// Add to the computer's own custom dictionary (a local file kept by Windows).
    #[allow(unused_variables)]
    pub fn add(&self, word: &str) {
        #[cfg(windows)]
        unsafe {
            if let Some(chk) = &self.checker {
                let _ = chk.Add(&HSTRING::from(word));
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
