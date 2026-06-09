//! `careerbot profile` command family.

use super::CommandError;
use crate::agent::{Attachment, AttachmentKind, Cost, ToolKit, prompts};
use crate::runtime::Runtime;
use crate::tools::ToolError;
use std::path::{Path, PathBuf};

/// Return the absolute path the `profile --edit` flow should open.
pub fn profile_path(rt: &Runtime) -> PathBuf {
    rt.paths.memory_dir().join("profile.md")
}

/// Read `profile.md`. Returns [`CommandError::NotFound`] when the file
/// hasn't been written yet, with a hint message the CLI can surface.
pub async fn show(rt: &Runtime) -> Result<String, CommandError> {
    match rt.tools.read_profile().await {
        Ok(s) => Ok(s),
        Err(ToolError::Io(io)) if io.kind() == std::io::ErrorKind::NotFound => {
            Err(CommandError::NotFound {
                what: "profile.md".into(),
            })
        }
        Err(e) => Err(e.into()),
    }
}

/// Output of a successful `profile --from-resume` run.
#[derive(Debug, Clone)]
pub struct FromResumeOutput {
    /// The assistant's closing text (typically a short summary).
    pub text: String,
    /// How many tool calls the agent made.
    pub tool_calls: usize,
    pub cost: Option<Cost>,
}

/// Read the resume at `path` and run the `profile_init` agent. PDFs go
/// to the LLM as a native attachment (Anthropic `document` block /
/// Claude Code `--add-dir`); text/markdown is included inline. The
/// agent is expected to call `write_profile`, so on success
/// `profile.md` is already on disk.
pub async fn from_resume(rt: &Runtime, path: &Path) -> Result<FromResumeOutput, CommandError> {
    if !path.exists() {
        return Err(CommandError::NotFound {
            what: format!("resume file {}", path.display()),
        });
    }

    let (prompt, attachments) = if is_pdf(path) {
        let prompt = format!(
            "The user's resume is the attached PDF (located at `{}`). \
             Read it and produce a profile.md via write_profile.",
            path.display()
        );
        let attachment = Attachment {
            path: path.to_path_buf(),
            kind: AttachmentKind::Pdf,
        };
        (prompt, vec![attachment])
    } else {
        let resume = std::fs::read_to_string(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::InvalidData {
                CommandError::InvalidInput(format!(
                    "resume file {} is not valid UTF-8; supported formats: \
                     .txt, .md, .pdf",
                    path.display()
                ))
            } else {
                CommandError::Io(e)
            }
        })?;
        (format!("Resume contents:\n\n{resume}"), Vec::new())
    };

    let driver = rt.build_driver()?;
    let toolkit = ToolKit::in_process(rt.tools.clone());
    let result = driver
        .run(
            prompt,
            prompts::PROFILE_INIT.to_string(),
            toolkit,
            None,
            "profile_init",
            &attachments,
        )
        .await?;

    Ok(FromResumeOutput {
        text: result.text,
        tool_calls: result.tool_calls.len(),
        cost: result.cost,
    })
}

fn is_pdf(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::paths::Paths;
    use serde_json::json;
    use tempfile::TempDir;
    use wiremock::matchers::{body_string_contains, method, path as wpath};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn rooted_with_mock() -> (TempDir, Runtime, MockServer) {
        let server = MockServer::start().await;
        let dir = TempDir::new().unwrap();
        let paths = Paths::rooted_at(dir.path().join("data"), dir.path().join("state"));
        // Pre-write the config so Runtime::open_at sees a usable driver.
        let mut cfg = Config::load(paths.config_file()).unwrap();
        cfg.set("agent.driver", toml::Value::String("anthropic_api".into()))
            .unwrap();
        cfg.set(
            "agent.anthropic_api.api_key",
            toml::Value::String("test-key".into()),
        )
        .unwrap();
        cfg.set(
            "agent.anthropic_api.base_url",
            toml::Value::String(server.uri()),
        )
        .unwrap();
        cfg.save().unwrap();
        let rt = Runtime::open_at(paths).await.unwrap();
        (dir, rt, server)
    }

    #[tokio::test]
    async fn show_missing_reports_not_found() {
        let dir = TempDir::new().unwrap();
        let paths = Paths::rooted_at(dir.path().join("data"), dir.path().join("state"));
        let rt = Runtime::open_at(paths).await.unwrap();
        let Err(err) = show(&rt).await else {
            panic!("expected NotFound");
        };
        assert!(matches!(err, CommandError::NotFound { .. }));
    }

    #[tokio::test]
    async fn show_returns_existing_profile() {
        let dir = TempDir::new().unwrap();
        let paths = Paths::rooted_at(dir.path().join("data"), dir.path().join("state"));
        let rt = Runtime::open_at(paths).await.unwrap();
        rt.tools.write_profile("# Profile\nHi.").await.unwrap();
        let got = show(&rt).await.unwrap();
        assert_eq!(got, "# Profile\nHi.");
    }

    #[tokio::test]
    async fn from_resume_drives_agent_and_writes_profile() {
        let (dir, rt, server) = rooted_with_mock().await;

        Mock::given(method("POST"))
            .and(wpath("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "content": [{
                    "type": "tool_use",
                    "id": "tu1",
                    "name": "write_profile",
                    "input": {"content": "# Profile\n\nSenior backend engineer."}
                }],
                "usage": {"input_tokens": 100, "output_tokens": 30}
            })))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(wpath("/v1/messages"))
            .and(body_string_contains("tool_result"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "content": [{"type": "text", "text": "Profile saved."}],
                "usage": {"input_tokens": 80, "output_tokens": 10}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let resume = dir.path().join("resume.txt");
        std::fs::write(&resume, "Senior backend engineer, 8 years.").unwrap();

        let output = from_resume(&rt, &resume).await.unwrap();

        assert_eq!(output.text, "Profile saved.");
        assert_eq!(output.tool_calls, 1);
        let cost = output.cost.unwrap();
        assert_eq!(cost.input_tokens, 180);
        assert_eq!(cost.output_tokens, 40);

        // The agent's tool call actually wrote the file.
        let written = rt.tools.read_profile().await.unwrap();
        assert!(written.contains("Senior backend engineer."));
    }

    #[tokio::test]
    async fn from_resume_reports_missing_file() {
        let (_dir, rt, _server) = rooted_with_mock().await;
        let Err(err) = from_resume(&rt, Path::new("/does/not/exist.txt")).await else {
            panic!("expected NotFound");
        };
        assert!(matches!(err, CommandError::NotFound { .. }));
    }

    #[tokio::test]
    async fn from_resume_reports_non_utf8_non_pdf_resume() {
        let (dir, rt, _server) = rooted_with_mock().await;
        // Non-PDF binary file (e.g. someone pointed at a .docx).
        let resume = dir.path().join("resume.docx");
        std::fs::write(&resume, [0xC0, 0xC1, 0xFF, 0xFE]).unwrap();
        let Err(err) = from_resume(&rt, &resume).await else {
            panic!("expected InvalidInput");
        };
        assert!(matches!(err, CommandError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn from_resume_with_pdf_attaches_the_file() {
        let (dir, rt, server) = rooted_with_mock().await;
        let pdf = dir.path().join("resume.pdf");
        std::fs::write(&pdf, b"%PDF-1.4\nstub").unwrap();

        // The driver should send a document content block carrying the
        // base64-encoded PDF bytes.
        Mock::given(method("POST"))
            .and(wpath("/v1/messages"))
            .and(body_string_contains("\"type\":\"document\""))
            .and(body_string_contains("\"media_type\":\"application/pdf\""))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "content": [{
                    "type": "tool_use", "id": "tu1", "name": "write_profile",
                    "input": {"content": "# Profile\n"}
                }],
                "usage": {"input_tokens": 100, "output_tokens": 5}
            })))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(wpath("/v1/messages"))
            .and(body_string_contains("tool_result"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "content": [{"type": "text", "text": "saved"}],
                "usage": {"input_tokens": 20, "output_tokens": 1}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let out = from_resume(&rt, &pdf).await.unwrap();
        assert_eq!(out.text, "saved");
    }
}
