use regex::Regex;

pub fn inject_before_the_end_of_body(
    content: &str,
    payload: &str,
) -> Result<String, anyhow::Error> {
    let regex = Regex::new(r#"</body>"#)?;

    let data = regex
        .replace(content, format!("{payload}</body>"))
        .to_string();

    Ok(data)
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    const CONTENT: &str = indoc! {r#"
        <!DOCTYPE html>
        <html>
        <body>
            <h1>My First Heading</h1>
            <p>My first paragraph.</p>
        </body>
        </html>
    "#};

    #[test]
    fn injection() {
        const EXPECTED: &str = indoc! {r#"
            <!DOCTYPE html>
            <html>
            <body>
                <h1>My First Heading</h1>
                <p>My first paragraph.</p>
            <p>Injected!</p></body>
            </html>
        "#};

        const PAYLOAD: &str = "<p>Injected!</p>";

        let actual = inject_before_the_end_of_body(CONTENT, PAYLOAD).unwrap();

        assert_eq!(actual, EXPECTED);
    }
}
