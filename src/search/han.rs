//! Text written without spaces between words: Chinese characters and
//! Japanese kana. The index holds each such character as one word, in its
//! simplified form, so that a phrase of them is found whatever divides it in
//! the text (a line end, a mark such as `¶`) and in either script.
//!
//! `unicode61` alone makes one word of a whole run of such characters, up to
//! the next space or punctuation mark.

use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_void};
use std::ptr;
use std::sync::OnceLock;

use rusqlite::{Connection, ffi};

/// The name of the tokenizer in the `tokenize` option of an FTS5 table. Its
/// arguments are the tokenizer it reads the words from, and that one's.
pub const TOKENIZER: &str = "lab_han";

/// Whether `c` is a word of its own: a Chinese character or a kana.
pub fn unspaced(c: char) -> bool {
    matches!(
        u32::from(c),
        0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x323AF
    )
}

/// The simplified form of a traditional Chinese character; any other
/// character as it is. One character for one, so it folds text for a
/// comparison and does not convert it: `乾` and `干` fold alike.
pub fn fold(c: char) -> char {
    static TABLE: OnceLock<HashMap<char, char>> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        include_str!("han_fold.txt")
            .lines()
            .filter(|line| !line.starts_with('#'))
            .filter_map(|line| {
                let mut chars = line.chars();
                Some((chars.next()?, chars.next()?))
            })
            .collect()
    });
    table.get(&c).copied().unwrap_or(c)
}

/// The ASCII form of a full-width digit or Latin letter, as Chinese and
/// Japanese print sets them (`１７６` is `176`); any other character as it is.
pub fn narrow(c: char) -> char {
    match c {
        '０'..='９' | 'Ａ'..='Ｚ' | 'ａ'..='ｚ' => {
            char::from_u32(u32::from(c) - 0xFEE0).unwrap_or(c)
        }
        _ => c,
    }
}

/// The words of `text` as a reader counts them: each run of characters
/// between spaces is one, and in a run with unspaced characters each of them
/// is one, with one more for letters or digits beside them.
pub fn word_count(text: &str) -> usize {
    text.split_whitespace()
        .map(|chunk| {
            let single = chunk.chars().filter(|c| unspaced(*c)).count();
            if single == 0 {
                return 1;
            }
            let other = chunk
                .split(unspaced)
                .any(|run| run.chars().any(char::is_alphanumeric));
            single + usize::from(other)
        })
        .sum()
}

/// The tokenizer below this one, and its instance.
struct Wrapped {
    parent: ffi::fts5_tokenizer,
    instance: *mut ffi::Fts5Tokenizer,
}

type Emit = unsafe extern "C" fn(*mut c_void, c_int, *const c_char, c_int, c_int, c_int) -> c_int;

/// One call of `tokenize`: the text, and where the words go.
struct Pass<'t> {
    text: &'t [u8],
    ctx: *mut c_void,
    emit: Emit,
}

/// FTS5 `xCreate`. `api` is the `fts5_api` that `register` passed.
unsafe extern "C" fn create(
    api: *mut c_void,
    args: *mut *const c_char,
    n_args: c_int,
    out: *mut *mut ffi::Fts5Tokenizer,
) -> c_int {
    let api = api.cast::<ffi::fts5_api>();
    // SAFETY: FTS5 passes `n_args` valid strings, and `api` outlives every
    // tokenizer of its connection.
    unsafe {
        let (name, rest, n_rest) = if n_args > 0 {
            (*args, args.add(1), n_args - 1)
        } else {
            (c"unicode61".as_ptr(), ptr::null_mut(), 0)
        };
        let mut parent = ffi::fts5_tokenizer {
            xCreate: None,
            xDelete: None,
            xTokenize: None,
        };
        let mut user = ptr::null_mut();
        let Some(find) = (*api).xFindTokenizer else {
            return ffi::SQLITE_ERROR;
        };
        let rc = find(api, name, &mut user, &mut parent);
        if rc != ffi::SQLITE_OK {
            return rc;
        }
        let Some(parent_create) = parent.xCreate else {
            return ffi::SQLITE_ERROR;
        };
        let mut instance = ptr::null_mut();
        let rc = parent_create(user, rest, n_rest, &mut instance);
        if rc != ffi::SQLITE_OK {
            return rc;
        }
        *out = Box::into_raw(Box::new(Wrapped { parent, instance })).cast();
    }
    ffi::SQLITE_OK
}

/// FTS5 `xDelete`.
unsafe extern "C" fn delete(tokenizer: *mut ffi::Fts5Tokenizer) {
    // SAFETY: `tokenizer` is the box that `create` leaked, deleted once.
    unsafe {
        let wrapped = Box::from_raw(tokenizer.cast::<Wrapped>());
        if let Some(parent_delete) = wrapped.parent.xDelete {
            parent_delete(wrapped.instance);
        }
    }
}

/// FTS5 `xTokenize`: the parent's words, each passed through `split`.
unsafe extern "C" fn tokenize(
    tokenizer: *mut ffi::Fts5Tokenizer,
    ctx: *mut c_void,
    flags: c_int,
    text: *const c_char,
    n_text: c_int,
    emit: Option<Emit>,
) -> c_int {
    // SAFETY: `tokenizer` is a live `Wrapped`, and `text` holds `n_text`
    // bytes for the length of this call.
    unsafe {
        let wrapped = &*tokenizer.cast::<Wrapped>();
        let (Some(parent_tokenize), Some(emit)) = (wrapped.parent.xTokenize, emit) else {
            return ffi::SQLITE_ERROR;
        };
        let bytes = match usize::try_from(n_text) {
            Ok(n) if n > 0 && !text.is_null() => std::slice::from_raw_parts(text.cast::<u8>(), n),
            _ => &[],
        };
        let mut pass = Pass {
            text: bytes,
            ctx,
            emit,
        };
        parent_tokenize(
            wrapped.instance,
            ptr::from_mut(&mut pass).cast(),
            flags,
            text,
            n_text,
            Some(split),
        )
    }
}

/// The parent's `xToken`: a word with unspaced characters goes on as one
/// word per such character, folded, and one per run of other characters
/// between them (`第176页` is `第`, `176`, `页`), with full-width digits and
/// letters as their ASCII forms. Each keeps its place in the text, so a
/// snippet shows the text as written.
unsafe extern "C" fn split(
    ctx: *mut c_void,
    tflags: c_int,
    token: *const c_char,
    n_token: c_int,
    start: c_int,
    end: c_int,
) -> c_int {
    // SAFETY: `ctx` is the `Pass` of the running `tokenize` call, and the
    // parent passes `n_token` bytes.
    unsafe {
        let pass = &*ctx.cast::<Pass>();
        let whole = || (pass.emit)(pass.ctx, tflags, token, n_token, start, end);
        let folded = std::slice::from_raw_parts(token.cast::<u8>(), n_token.max(0) as usize);
        if folded.is_ascii() {
            return whole();
        }
        let source = usize::try_from(start)
            .ok()
            .zip(usize::try_from(end).ok())
            .and_then(|(from, to)| pass.text.get(from..to))
            .and_then(|bytes| std::str::from_utf8(bytes).ok());
        let changes = |s: &&str| s.chars().any(|c| unspaced(c) || narrow(c) != c);
        let Some(source) = source.filter(changes) else {
            return whole();
        };
        // The parent's form of each run keeps its folding (case, diacritics).
        // The runs pair with the text's by their order.
        fn runs(s: &str) -> impl Iterator<Item = &str> {
            s.split(unspaced).filter(|run| !run.is_empty())
        }
        let mut parents = std::str::from_utf8(folded)
            .ok()
            .filter(|folded| runs(folded).count() == runs(source).count())
            .map(|folded| runs(folded).collect::<Vec<_>>().into_iter());
        let mut plain = |run: &str| -> String {
            match parents.as_mut().and_then(Iterator::next) {
                Some(parent) => parent.chars().map(narrow).collect(),
                None => run.chars().map(narrow).collect::<String>().to_lowercase(),
            }
        };
        let send = |word: &str, from: usize, to: usize| {
            (pass.emit)(
                pass.ctx,
                0,
                word.as_ptr().cast(),
                word.len() as c_int,
                start + from as c_int,
                start + to as c_int,
            )
        };
        let mut run: Option<usize> = None;
        let mut buffer = [0u8; 4];
        for (at, c) in source.char_indices() {
            if !unspaced(c) {
                run.get_or_insert(at);
                continue;
            }
            if let Some(from) = run.take() {
                let rc = send(&plain(&source[from..at]), from, at);
                if rc != ffi::SQLITE_OK {
                    return rc;
                }
            }
            let rc = send(fold(c).encode_utf8(&mut buffer), at, at + c.len_utf8());
            if rc != ffi::SQLITE_OK {
                return rc;
            }
        }
        match run {
            Some(from) => send(&plain(&source[from..]), from, source.len()),
            None => ffi::SQLITE_OK,
        }
    }
}

/// Adds the tokenizer to a connection. Every connection that reads or writes
/// `units_fts` needs it.
pub fn register(conn: &Connection) -> rusqlite::Result<()> {
    let failed = |rc: c_int, what: &str| {
        rusqlite::Error::SqliteFailure(ffi::Error::new(rc), Some(format!("{TOKENIZER}: {what}")))
    };
    // SAFETY: the calls follow "Extending FTS5" of the SQLite documentation:
    // `SELECT fts5(?1)` writes the connection's `fts5_api` to the bound
    // pointer, and `xCreateTokenizer` copies the three functions.
    unsafe {
        let db = conn.handle();
        let mut stmt = ptr::null_mut();
        let rc = ffi::sqlite3_prepare_v2(
            db,
            c"SELECT fts5(?1)".as_ptr(),
            -1,
            &mut stmt,
            ptr::null_mut(),
        );
        if rc != ffi::SQLITE_OK {
            return Err(failed(rc, "FTS5 is missing"));
        }
        let mut api: *mut ffi::fts5_api = ptr::null_mut();
        ffi::sqlite3_bind_pointer(
            stmt,
            1,
            ptr::from_mut(&mut api).cast(),
            c"fts5_api_ptr".as_ptr(),
            None,
        );
        ffi::sqlite3_step(stmt);
        let rc = ffi::sqlite3_finalize(stmt);
        if rc != ffi::SQLITE_OK || api.is_null() {
            return Err(failed(rc, "FTS5 gave no API"));
        }
        let Some(add) = (*api).xCreateTokenizer else {
            return Err(failed(ffi::SQLITE_ERROR, "FTS5 takes no tokenizer"));
        };
        let mut functions = ffi::fts5_tokenizer {
            xCreate: Some(create),
            xDelete: Some(delete),
            xTokenize: Some(tokenize),
        };
        let name = c"lab_han";
        debug_assert_eq!(name.to_str(), Ok(TOKENIZER));
        let rc = add(api, name.as_ptr(), api.cast(), &mut functions, None);
        if rc != ffi::SQLITE_OK {
            return Err(failed(rc, "not added"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        register(&conn).unwrap();
        conn.execute_batch(
            "CREATE VIRTUAL TABLE t USING fts5(body, \
             tokenize = 'porter lab_han unicode61 remove_diacritics 2');
             CREATE VIRTUAL TABLE v USING fts5vocab(t, 'row');",
        )
        .unwrap();
        conn
    }

    fn rows(conn: &Connection, sql: &str, query: &str) -> Vec<String> {
        conn.prepare(sql)
            .unwrap()
            .query_map([query], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    #[test]
    fn each_chinese_character_is_a_word_in_its_simplified_form() {
        let conn = table();
        conn.execute(
            "INSERT INTO t (body) VALUES (?1)",
            ["學而時習之，第176頁 Retrieval ２００４年 ＪＯＵＲＮＡＬ"],
        )
        .unwrap();
        let words = rows(&conn, "SELECT term FROM v WHERE ?1 = '' ORDER BY term", "");
        assert_eq!(
            words,
            [
                "176", "2004", "journal", "retriev", "之", "习", "学", "年", "时", "第", "而", "页"
            ]
        );
    }

    #[test]
    fn a_phrase_matches_in_either_script_and_across_a_line_end() {
        let conn = table();
        conn.execute(
            "INSERT INTO t (body) VALUES (?1)",
            ["子曰學而時習之不亦説乎有朋自逺方来不亦¶\n樂乎人不知而不愠"],
        )
        .unwrap();
        let find = |query: &str| {
            rows(
                &conn,
                "SELECT snippet(t, 0, '[', ']', '…', 64) FROM t WHERE t MATCH ?1",
                &format!("\"{query}\""),
            )
        };
        // Simplified for traditional, and the text as written in the snippet.
        assert_eq!(find("学而时习之").len(), 1);
        assert!(find("學而時習之")[0].contains("[學而時習之]"));
        // Over the mark and the line end.
        let found = find("有朋自逺方来不亦樂乎");
        assert_eq!(found.len(), 1);
        assert!(found[0].contains("不亦¶\n樂乎"), "{found:?}");
        // The characters in another order are another phrase.
        assert!(find("习时而学").is_empty());
        assert!(find("乐不亦").is_empty());
    }

    #[test]
    fn kana_and_mixed_words_split_and_keep_the_parent_folding() {
        let conn = table();
        conn.execute(
            "INSERT INTO t (body) VALUES (?1)",
            ["BERT模型とCafé学 ひらがなカタカナ"],
        )
        .unwrap();
        let find = |query: &str| rows(&conn, "SELECT body FROM t WHERE t MATCH ?1", query).len();
        // A Latin run beside Chinese is the word it is alone, without its
        // diacritics.
        assert_eq!(find("bert"), 1);
        assert_eq!(find("cafe"), 1);
        assert_eq!(find("\"BERT 模型\""), 1);
        assert_eq!(find("\"ＢＥＲＴ模型\""), 1);
        // Kana: one word per character, a phrase in order.
        assert_eq!(find("\"らがな\""), 1);
        assert_eq!(find("\"タカナ\""), 1);
        assert_eq!(find("\"がらな\""), 0);
    }

    #[test]
    fn other_words_stay_whole() {
        let conn = table();
        conn.execute("INSERT INTO t (body) VALUES (?1)", ["Café naïve retrieval"])
            .unwrap();
        let words = rows(&conn, "SELECT term FROM v WHERE ?1 = '' ORDER BY term", "");
        assert_eq!(words, ["cafe", "naiv", "retriev"]);
    }

    #[test]
    fn folding_twice_changes_nothing() {
        for line in include_str!("han_fold.txt").lines() {
            let Some(c) = line.chars().next().filter(|c| *c != '#') else {
                continue;
            };
            assert_ne!(fold(c), c, "{c} folds to itself");
            assert_eq!(fold(fold(c)), fold(c), "{c} folds twice");
        }
        assert_eq!((fold('學'), fold('学'), fold('a')), ('学', '学', 'a'));
    }

    #[test]
    fn counts_each_unspaced_character_as_a_word() {
        assert_eq!(word_count("two words"), 2);
        assert_eq!(word_count("學而時習之，不亦説乎"), 9);
        assert_eq!(word_count("第176頁 and more"), 5);
        // Other text counts as it did: each run between spaces.
        assert_eq!(word_count("| 12 | 34 |"), 5);
        assert_eq!(word_count("ok - done"), 3);
        // A message of three characters is not short.
        assert_eq!(word_count("好的。"), 2);
        assert_eq!(word_count("可以了"), 3);
    }
}
