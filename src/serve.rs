use std::path::Path;

use crate::inject::inject_before_the_end_of_body;

pub async fn serve_file(
    target_dir: impl AsRef<Path>,
    path_in_request: &str,
    payload: &str,
) -> anyhow::Result<Option<Vec<u8>>> {
    let target_dir = target_dir.as_ref();

    let mut path = path_in_request;

    if path == "/" || path.is_empty() {
        path = "/index.html";
    }

    let path = path.strip_prefix('/').unwrap_or(path);

    let path_in_target_dir = target_dir.join(path);

    if !path_in_target_dir.exists() {
        return Ok(None);
    }

    let body = if path.ends_with(".html") {
        log::info!(
            "Serving HTML requested at {path} with {path_in_target_dir:?}"
        );

        let content = tokio::fs::read_to_string(path_in_target_dir).await?;

        let data = inject_before_the_end_of_body(content.as_str(), payload)?;

        data.bytes().collect()
    } else {
        log::info!("Serving a non HTML file requested as {path_in_request} with {path_in_target_dir:?}");

        tokio::fs::read(path_in_target_dir).await?
    };

    Ok(Some(body))
}

#[cfg(test)]
mod tests {
    use indoc::indoc;
    use tempfile::TempDir;

    use super::*;

    const INDEX: &str = indoc! {r#"
        <!DOCTYPE html>
        <html>
        <body>
            <h1>My First Heading</h1>
            <p>My first paragraph.</p>
        </body>
        </html>
    "#};

    const INDEX_CSS: &str = indoc! {r#"
        body {
            color: red;
        }
    "#};

    async fn prepare_directory() -> anyhow::Result<TempDir> {
        let temp_dir = TempDir::new()?;

        tokio::fs::write(temp_dir.path().join("index.html"), INDEX).await?;
        tokio::fs::write(temp_dir.path().join("index.css"), INDEX_CSS).await?;

        Ok(temp_dir)
    }

    #[tokio::test]
    async fn serving_a_missing_file_returns_none() -> anyhow::Result<()> {
        let temp_dir = prepare_directory().await?;

        let served = serve_file(temp_dir.path(), "favicon.ico", "").await?;

        assert!(served.is_none());

        Ok(())
    }

    #[tokio::test]
    async fn serving_at_root_returns_index_html() -> anyhow::Result<()> {
        let temp_dir = prepare_directory().await?;

        let expected: Vec<u8> = INDEX.bytes().collect();

        let served = serve_file(temp_dir.path(), "index.html", "")
            .await?
            .unwrap();
        assert_eq!(served, expected);

        let served = serve_file(temp_dir.path(), "/", "").await?.unwrap();
        assert_eq!(served, expected);

        let served = serve_file(temp_dir.path(), "", "").await?.unwrap();
        assert_eq!(served, expected);

        Ok(())
    }

    #[tokio::test]
    async fn serving_an_existing_file_returns_it() -> anyhow::Result<()> {
        let temp_dir = prepare_directory().await?;

        let served =
            serve_file(temp_dir.path(), "index.css", "").await?.unwrap();

        let expected: Vec<u8> = INDEX_CSS.bytes().collect();

        assert_eq!(served, expected);

        Ok(())
    }
}
