fn main() {
    let text = "# a header\n\nwrap = true\n# about blame\nblame = false\n";
    let mut doc: toml_edit::DocumentMut = text.parse().unwrap();
    let prefix = doc
        .as_table()
        .get_key_value("wrap")
        .and_then(|(key, _)| key.leaf_decor().prefix().cloned());
    println!("prefix of wrap: {prefix:?}");
    doc.remove("wrap");
    if let Some(prefix) = prefix
        && let Some((_, next)) = doc.as_table_mut().iter_mut().next()
    {
        let _ = next;
    }
    // move it onto the next key
    let first: Option<String> = doc.as_table().iter().next().map(|(k, _)| k.to_string());
    if let (Some(prefix), Some(first)) = (prefix, first)
        && let Some(mut key) = doc.as_table_mut().key_mut(&first)
    {
        let existing = key.leaf_decor().prefix().cloned().unwrap_or_default();
        key.leaf_decor_mut()
            .set_prefix(format!("{}{}", prefix.as_str().unwrap_or(""), existing.as_str().unwrap_or("")));
    }
    println!("---\n{doc}");
}
