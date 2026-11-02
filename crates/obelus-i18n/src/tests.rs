use super::*;

/// The tests that say something in a language take turns: which one is
/// spoken is the process's.
static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn in_language<T>(language: Language, said: impl FnOnce() -> T) -> T {
    let _turn = TURN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    speak(language.word());
    let said = said();
    speak("en");
    said
}

fn language(word: &str, files: &[(&str, &str)]) -> generate::Language {
    generate::Language {
        word: word.to_string(),
        files: files
            .iter()
            .map(|(stem, source)| generate::File {
                stem: (*stem).to_string(),
                path: format!("locales/{word}/{stem}.ftl"),
                source: (*source).to_string(),
            })
            .collect(),
    }
}

fn refused(languages: &[generate::Language]) -> Vec<String> {
    match generate::generate(languages) {
        Ok(_) => Vec::new(),
        Err(wrong) => wrong,
    }
}

/// Broken deliberately by putting `Language::ALL` in the other order: every
/// English word came out in Chinese.
#[test]
fn every_language_is_where_the_build_keeps_its_messages() {
    let words: Vec<&str> = Language::ALL
        .iter()
        .map(|language| language.word())
        .collect();
    assert_eq!(words, WORDS);
    assert_eq!(&CHOICES[1..], WORDS);
}

/// Broken deliberately by giving `Catalogue::of` back the isolating marks
/// (taking out `set_use_isolating(false)`): the sentence came out with
/// U+2068 and U+2069 round the version, two characters a terminal draws.
#[test]
fn a_variable_goes_in_with_nothing_round_it() {
    let said = in_language(Language::English, || welcome::newer_is_out("0.19.0"));
    assert_eq!(said, "v0.19.0 is out");
}

/// Broken deliberately by making `speak` ignore what it is told: the
/// Chinese came out in English.
#[test]
fn each_language_says_its_own_words() {
    assert_eq!(
        in_language(Language::English, settings::group_appearance),
        "Appearance"
    );
    assert_eq!(
        in_language(Language::SimplifiedChinese, settings::group_appearance),
        "外观"
    );
    assert_eq!(
        in_language(Language::SimplifiedChinese, || welcome::newer_is_out(
            "0.19.0"
        )),
        "v0.19.0 已发布"
    );
}

/// Broken deliberately by making `auto` English whatever the system said
/// -- which a test cannot reach through `hear_the_system`, since the
/// system is the machine's, so the pure half is what is held here.
#[test]
fn a_locale_names_a_language_by_its_first_part() {
    for (locale, language) in [
        ("zh-CN", Language::SimplifiedChinese),
        ("zh_CN.UTF-8", Language::SimplifiedChinese),
        ("zh-Hans-CN", Language::SimplifiedChinese),
        ("zh-TW", Language::SimplifiedChinese),
        ("ZH", Language::SimplifiedChinese),
        ("en-GB", Language::English),
        ("fr_FR.UTF-8", Language::English),
        ("", Language::English),
        ("C", Language::English),
    ] {
        assert_eq!(Language::of_the_locale(locale), language, "{locale}");
    }
}

/// Broken deliberately by giving `speak` an unknown word as the system's
/// language rather than English.
#[test]
fn a_word_that_names_nothing_is_english() {
    let said = in_language(Language::SimplifiedChinese, || {
        speak("klingon");
        settings::group_appearance()
    });
    assert_eq!(said, "Appearance");
}

/// A plural is the language's: English has two forms and the number picks.
///
/// Broken deliberately by making a number a word on its way into an `Arg`
/// (`FluentValue::from(number.to_string())` in `numbers!`): `[one]` never
/// matched and one came out as `1 files`.
#[test]
fn a_number_picks_its_plural_in_the_language() {
    let source = "files = { $n ->\n    [one] { $n } file\n   *[other] { $n } files\n}\n";
    let mut bundle = FluentBundle::new_concurrent(vec!["en".parse().expect("a tag")]);
    bundle.set_use_isolating(false);
    bundle
        .add_resource(FluentResource::try_new(source.to_string()).expect("parses"))
        .expect("adds");
    let files = |n: usize| {
        let mut args = FluentArgs::new();
        let Arg(value) = Arg::from(n);
        args.set("n", value);
        say(&bundle, "en", "files", Some(&args))
    };
    assert_eq!(files(1), "1 file");
    assert_eq!(files(2), "2 files");
}

/// Broken deliberately by skipping the check for missing messages: the
/// build went through with Chinese one short.
#[test]
fn a_language_without_a_message_stops_the_build() {
    let wrong = refused(&[
        language("en", &[("one", "a = A\nb = B\n")]),
        language("zh-CN", &[("one", "a = 甲\n")]),
    ]);
    assert_eq!(wrong, ["zh-CN/one.ftl: b is missing"]);
}

/// Broken deliberately by skipping the check the other way round.
#[test]
fn a_message_the_first_does_not_have_stops_the_build() {
    let wrong = refused(&[
        language("en", &[("one", "a = A\n")]),
        language("zh-CN", &[("one", "a = 甲\nc = 丙\n")]),
    ]);
    assert_eq!(wrong, ["zh-CN/one.ftl: c is not in en"]);
}

/// Broken deliberately by comparing the variables with `==` the wrong way
/// round (the first's against the translation's): a translation that
/// leaves one out -- which is allowed, Chinese drops a plural's number
/// often enough -- was refused, and this one was let through.
#[test]
fn a_variable_nobody_passes_stops_the_build() {
    let wrong = refused(&[
        language("en", &[("one", "a = { $n } things\nb = { $n } things\n")]),
        language("zh-CN", &[("one", "a = { $m } 个\nb = 很多\n")]),
    ]);
    assert_eq!(wrong, ["zh-CN/one.ftl: a uses $m, which en does not pass"]);
}

/// Broken deliberately by not following a message a message names: `b`
/// took nothing and came out with a hole where `$n` was.
#[test]
fn a_message_takes_what_the_messages_it_names_take() {
    let generated = generate::generate(&[language(
        "en",
        &[("one", "a = { $n } things\nb = All { a }\n")],
    )])
    .expect("generates");
    assert!(
        generated.contains("pub fn b(n: impl Into<crate::Arg>) -> String"),
        "{generated}"
    );
}

/// Broken deliberately by not looking for parser errors: the junk line was
/// dropped and the build went through.
#[test]
fn a_file_that_will_not_parse_stops_the_build() {
    let wrong = refused(&[language("en", &[("one", "a = A\nthis is not a message\n")])]);
    assert_eq!(wrong.len(), 1, "{wrong:?}");
    assert!(wrong[0].starts_with("en/one.ftl:2:"), "{wrong:?}");
}

/// Broken deliberately by letting `is_a_name` take anything: `type` became
/// `pub fn type()`, which does not compile anywhere near the file that
/// caused it.
#[test]
fn a_name_rust_cannot_have_stops_the_build() {
    let wrong = refused(&[language("en", &[("one", "type = A\nok-name = B\n")])]);
    assert_eq!(
        wrong,
        ["en/one.ftl: type has to be a word in lowercase and hyphens"]
    );
}

/// A file is a bundle of its own, so one name in two files is two
/// messages -- and one name twice in a file is a mistake.
///
/// Broken deliberately by collecting a file's messages into a map without
/// asking whether the name was already there: the second `a` went in over
/// the first, and the build said nothing.
#[test]
fn a_message_said_twice_in_a_file_stops_the_build() {
    let wrong = refused(&[language("en", &[("one", "a = A\na = B\n")])]);
    assert_eq!(wrong, ["en/one.ftl: a is said twice"]);
    let apart = refused(&[language("en", &[("one", "a = A\n"), ("two", "a = B\n")])]);
    assert!(apart.is_empty(), "{apart:?}");
}

/// A table that names its rows asks for each by that name, in its own file
/// and no other. That one file's `named` cannot answer for another's is the
/// compiler's: a module's arms call its own functions, and only those are
/// in scope.
///
/// Broken deliberately by writing every `named` as the empty one: the
/// commands and the settings asked by name and got nothing.
#[test]
fn a_message_is_named_in_its_own_file() {
    let said = in_language(Language::SimplifiedChinese, || {
        (
            commands::named("open-file"),
            settings::named("theme"),
            palette::named("theme"),
        )
    });
    assert_eq!(said, (Some("打开文件"), Some("主题"), None));
}

/// Broken deliberately by checking the files of a language only by the
/// first's list: a file only Chinese had went unnoticed.
#[test]
fn a_file_one_language_has_and_another_does_not_stops_the_build() {
    let wrong = refused(&[
        language("en", &[("one", "a = A\n")]),
        language("zh-CN", &[("one", "a = 甲\n"), ("two", "b = 乙\n")]),
    ]);
    assert_eq!(wrong, ["zh-CN/two.ftl: en has no such file"]);
}
