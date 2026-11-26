use std::time::{Duration, Instant};

use eframe::egui::{self, text::LayoutJob, Color32, FontId, Margin, RichText, TextFormat};

use crate::theme::ThemeTokens;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageBlock {
    Text(String),
    Code { lang: String, code: String },
}

/// Splits a markdown message into prose and fenced code blocks.
/// Supports unclosed fences so code blocks render immediately while streaming.
pub fn parse_message_blocks(input: &str) -> Vec<MessageBlock> {
    let mut blocks = Vec::new();
    let mut in_code = false;
    let mut code_lang = String::new();
    let mut buf: Vec<&str> = Vec::new();

    for line in input.lines() {
        let trimmed = line.trim_start();
        if let Some(after_fence) = trimmed.strip_prefix("```") {
            if !in_code {
                flush_text(&mut blocks, &mut buf);
                code_lang = after_fence.trim().to_string();
                in_code = true;
            } else {
                flush_code(&mut blocks, &mut code_lang, &mut buf);
                in_code = false;
            }
        } else {
            buf.push(line);
        }
    }

    if in_code {
        flush_code(&mut blocks, &mut code_lang, &mut buf);
    } else {
        flush_text(&mut blocks, &mut buf);
    }

    blocks
}

fn flush_text(blocks: &mut Vec<MessageBlock>, buf: &mut Vec<&str>) {
    let text = buf.join("\n");
    let trimmed = text.trim();
    if !trimmed.is_empty() {
        blocks.push(MessageBlock::Text(trimmed.to_string()));
    }
    buf.clear();
}

fn flush_code(blocks: &mut Vec<MessageBlock>, lang: &mut String, buf: &mut Vec<&str>) {
    let code = buf.join("\n").trim_end_matches('\n').to_string();
    blocks.push(MessageBlock::Code {
        lang: if lang.is_empty() {
            "code".to_string()
        } else {
            lang.clone()
        },
        code,
    });
    lang.clear();
    buf.clear();
}

/// Renders prose with lightweight inline `code` and **bold** formatting.
fn build_inline_layout_job(text: &str, t: &ThemeTokens, wrap_width: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap_width;

    let normal_fmt = TextFormat {
        font_id: FontId::proportional(13.0),
        color: t.text_primary,
        ..Default::default()
    };
    let bold_fmt = TextFormat {
        font_id: FontId::proportional(13.0),
        color: Color32::WHITE,
        ..Default::default()
    };
    let code_fmt = TextFormat {
        font_id: FontId::monospace(12.0),
        color: t.accent,
        background: Color32::from_rgb(16, 16, 20),
        ..Default::default()
    };

    let mut rest = text;
    while !rest.is_empty() {
        let next_tick = rest.find('`');
        let next_bold = rest.find("**");

        match (next_tick, next_bold) {
            (Some(ti), None) => {
                if ti > 0 {
                    job.append(&rest[..ti], 0.0, normal_fmt.clone());
                }
                let after = &rest[ti + 1..];
                if let Some(end) = after.find('`') {
                    job.append(&after[..end], 0.0, code_fmt.clone());
                    rest = &after[end + 1..];
                } else {
                    job.append(&rest[ti..], 0.0, normal_fmt.clone());
                    break;
                }
            }
            (Some(ti), Some(bi)) if ti < bi => {
                if ti > 0 {
                    job.append(&rest[..ti], 0.0, normal_fmt.clone());
                }
                let after = &rest[ti + 1..];
                if let Some(end) = after.find('`') {
                    job.append(&after[..end], 0.0, code_fmt.clone());
                    rest = &after[end + 1..];
                } else {
                    job.append(&rest[ti..], 0.0, normal_fmt.clone());
                    break;
                }
            }
            (_, Some(bi)) => {
                if bi > 0 {
                    job.append(&rest[..bi], 0.0, normal_fmt.clone());
                }
                let after = &rest[bi + 2..];
                if let Some(end) = after.find("**") {
                    job.append(&after[..end], 0.0, bold_fmt.clone());
                    rest = &after[end + 2..];
                } else {
                    job.append(&rest[bi..], 0.0, normal_fmt.clone());
                    break;
                }
            }
            (None, None) => {
                job.append(rest, 0.0, normal_fmt.clone());
                break;
            }
        }
    }

    job
}

/// Renders an assistant message with formatted code blocks and 1-click Copy buttons.
pub fn render_assistant_blocks(
    ui: &mut egui::Ui,
    msg_id: &str,
    text: &str,
    max_width: f32,
    t: &ThemeTokens,
    copied_feedback: &mut Option<(String, Instant)>,
) {
    let blocks = parse_message_blocks(text);
    let content_w = (max_width - 20.0).max(120.0);

    for (idx, block) in blocks.iter().enumerate() {
        if idx > 0 {
            ui.add_space(6.0);
        }
        match block {
            MessageBlock::Text(prose) => {
                let job = build_inline_layout_job(prose, t, content_w);
                ui.add(egui::Label::new(job).selectable(true));
            }
            MessageBlock::Code { lang, code } => {
                let block_key = format!("{msg_id}_code_{idx}");
                render_code_block(ui, &block_key, lang, code, content_w, t, copied_feedback);
            }
        }
    }
}

fn render_code_block(
    ui: &mut egui::Ui,
    block_key: &str,
    lang: &str,
    code: &str,
    width: f32,
    t: &ThemeTokens,
    copied_feedback: &mut Option<(String, Instant)>,
) {
    let is_copied = copied_feedback
        .as_ref()
        .map(|(k, ts)| k == block_key && ts.elapsed() < Duration::from_secs(2))
        .unwrap_or(false);

    let code_bg = Color32::from_rgb(14, 14, 18);
    let header_bg = Color32::from_rgb(28, 28, 34);
    let code_fg = Color32::from_rgb(228, 232, 240);

    egui::Frame::none()
        .fill(code_bg)
        .stroke(t.hairline_border)
        .rounding(t.rounding_control)
        .show(ui, |ui| {
            ui.set_width(width);
            ui.vertical(|ui| {
                // Header bar: language label + Copy button
                egui::Frame::none()
                    .fill(header_bg)
                    .inner_margin(Margin::symmetric(8.0, 4.0))
                    .show(ui, |ui| {
                        ui.set_width((width - 16.0).max(80.0));
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(lang)
                                    .font(FontId::monospace(11.0))
                                    .color(t.accent),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let (label, color) = if is_copied {
                                        ("Copied ✓", t.status_live)
                                    } else {
                                        ("Copy", t.text_primary)
                                    };
                                    let btn = egui::Button::new(
                                        RichText::new(label).color(color).size(11.0),
                                    )
                                    .fill(t.bg_surface_subtle)
                                    .stroke(t.hairline_subtle)
                                    .rounding(t.rounding_badge);

                                    if ui
                                        .add(btn)
                                        .on_hover_text("Copy code to clipboard")
                                        .clicked()
                                    {
                                        ui.ctx().copy_text(code.to_string());
                                        *copied_feedback =
                                            Some((block_key.to_string(), Instant::now()));
                                    }
                                },
                            );
                        });
                    });

                // Horizontally scrollable monospace code body
                egui::Frame::none()
                    .inner_margin(Margin::symmetric(8.0, 6.0))
                    .show(ui, |ui| {
                        egui::ScrollArea::horizontal()
                            .id_salt(block_key)
                            .max_width((width - 16.0).max(80.0))
                            .show(ui, |ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(code)
                                            .font(FontId::monospace(12.0))
                                            .color(code_fg),
                                    )
                                    .selectable(true)
                                    .wrap_mode(egui::TextWrapMode::Extend),
                                );
                            });
                    });
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_prose_and_python_code_block() {
        let sample =
            "[happy] Here is your donut:\n\n```python\nimport math\nprint('donut')\n```\n\nEnjoy!";
        let blocks = parse_message_blocks(sample);
        assert_eq!(
            blocks,
            vec![
                MessageBlock::Text("[happy] Here is your donut:".to_string()),
                MessageBlock::Code {
                    lang: "python".to_string(),
                    code: "import math\nprint('donut')".to_string(),
                },
                MessageBlock::Text("Enjoy!".to_string()),
            ]
        );
    }

    #[test]
    fn parses_streaming_unclosed_code_fence() {
        let streaming = "Starting code:\n```python\nimport numpy as np\nx = 1";
        let blocks = parse_message_blocks(streaming);
        assert_eq!(
            blocks,
            vec![
                MessageBlock::Text("Starting code:".to_string()),
                MessageBlock::Code {
                    lang: "python".to_string(),
                    code: "import numpy as np\nx = 1".to_string(),
                },
            ]
        );
    }

    #[test]
    fn defaults_empty_fence_language_to_code() {
        let sample = "```\necho hi\n```";
        let blocks = parse_message_blocks(sample);
        assert_eq!(
            blocks,
            vec![MessageBlock::Code {
                lang: "code".to_string(),
                code: "echo hi".to_string(),
            }]
        );
    }
}
