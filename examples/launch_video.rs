//! Renders zappp's launch video, frame by frame, as code:
//!
//!   cargo run --example launch_video -- dist/zappp-launch.mp4
//!   cargo run --example launch_video -- --still 9.5 frame.png
//!
//! Each frame is an SVG string built from the timeline below, rasterized with
//! resvg and streamed as raw RGBA into ffmpeg (no frames are written to disk).

use std::{
    f32::consts::TAU,
    io::Write as _,
    path::Path,
    process::{Command, Stdio},
    sync::Arc,
};

use resvg::{tiny_skia, usvg};

const W: f32 = 1920.;
const H: f32 = 1080.;
const FPS: f32 = 30.;
const DURATION: f32 = 30.;

const BG_TOP: &str = "#121218";
const BG_BOTTOM: &str = "#07070A";
const ACCENT: &str = "#0A84FF";
const RED: &str = "#FF453A";
const SANS: &str = "Helvetica Neue, Helvetica, Arial, sans-serif";
const MONO: &str = "Menlo, Monaco, monospace";

// ── Timeline (seconds) ──────────────────────────────────────────────────

/// Scene windows; neighbours overlap by `XFADE` for cross-fades.
const LOGO: (f32, f32) = (0.0, 3.4);
const TAGLINE: (f32, f32) = (3.2, 6.6);
const DEMO: (f32, f32) = (6.4, 17.4);
const STATS: (f32, f32) = (17.2, 21.8);
const MODELS: (f32, f32) = (21.6, 25.8);
const END: (f32, f32) = (25.6, DURATION + 1.);
const XFADE: f32 = 0.45;

// Inside the demo.
const KEY_DOWN: f32 = 7.6;
const KEY_UP: f32 = 13.4;
const TRANSCRIPT: &str = "Hey team — the new build is ready for testing. Latency is under two hundred milliseconds, so let's ship it Friday.";

// ── Easing ──────────────────────────────────────────────────────────────

fn clamp01(x: f32) -> f32 {
    x.clamp(0., 1.)
}

/// Progress of `t` through [a, b], 0..=1.
fn seg(t: f32, a: f32, b: f32) -> f32 {
    clamp01((t - a) / (b - a))
}

fn ease_out(x: f32) -> f32 {
    1. - (1. - clamp01(x)).powi(3)
}

fn ease_in_out(x: f32) -> f32 {
    let x = clamp01(x);
    if x < 0.5 { 4. * x * x * x } else { 1. - (-2. * x + 2.).powi(3) / 2. }
}

/// Underdamped spring from 0 to 1 (a little overshoot), `x` in seconds.
fn spring(x: f32) -> f32 {
    if x <= 0. {
        return 0.;
    }
    1. - (-7. * x).exp() * (11. * x).cos()
}

fn lerp(a: f32, b: f32, x: f32) -> f32 {
    a + (b - a) * x
}

/// Opacity of a scene: fades in at its start and out at its end.
fn scene_alpha(t: f32, (a, b): (f32, f32)) -> f32 {
    seg(t, a, a + XFADE).min(1. - seg(t, b - XFADE, b))
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn text(x: f32, y: f32, size: f32, weight: u32, fill: &str, opacity: f32, anchor: &str, family: &str, body: &str) -> String {
    format!(
        r#"<text x="{x:.1}" y="{y:.1}" font-family="{family}" font-size="{size:.1}" font-weight="{weight}" fill="{fill}" fill-opacity="{opacity:.3}" text-anchor="{anchor}">{}</text>"#,
        esc(body)
    )
}

// ── Scenes ──────────────────────────────────────────────────────────────

struct Assets {
    icon_uri: String,
}

fn icon(assets: &Assets, cx: f32, cy: f32, size: f32, opacity: f32) -> String {
    format!(
        r#"<image href="{}" x="{:.1}" y="{:.1}" width="{size:.1}" height="{size:.1}" opacity="{opacity:.3}"/>"#,
        assets.icon_uri,
        cx - size / 2.,
        cy - size / 2.
    )
}

fn glow(cx: f32, cy: f32, r: f32, opacity: f32) -> String {
    format!(
        r#"<circle cx="{cx:.1}" cy="{cy:.1}" r="{r:.1}" fill="url(#glow)" opacity="{opacity:.3}"/>"#
    )
}

fn scene_logo(t: f32, a: &Assets) -> String {
    let alpha = scene_alpha(t, LOGO);
    let s = spring(t - 0.15);
    let size = 260. * lerp(0.55, 1., s);
    let word = ease_out(seg(t, 0.75, 1.5));
    let rise = (1. - word) * 24.;
    format!(
        "{}{}{}",
        glow(W / 2., H / 2. - 60., 520., alpha * clamp01(s) * 0.9),
        icon(a, W / 2., H / 2. - 70., size, alpha * clamp01(s * 1.4)),
        text(W / 2., H / 2. + 150. + rise, 96., 700, "#FFFFFF", alpha * word, "middle", SANS, "zappp"),
    )
}

fn scene_tagline(t: f32) -> String {
    let alpha = scene_alpha(t, TAGLINE);
    // Widths of each word at 104 px Helvetica Neue Bold, so the phrase is
    // centred as a whole with even gaps.
    let words = [("Hold a key.", 3.45, 560.), ("Talk.", 4.15, 255.), ("Let go.", 4.85, 365.)];
    let gap = 44.;
    let total: f32 = words.iter().map(|w| w.2).sum::<f32>() + gap * (words.len() - 1) as f32;
    let mut x = (W - total) / 2.;
    let mut out = String::new();
    for (word, at, width) in words {
        let p = ease_out(seg(t, at, at + 0.55));
        let color = if word == "Talk." { ACCENT } else { "#FFFFFF" };
        out += &text(x, H / 2. + 30. + (1. - p) * 40., 104., 700, color, alpha * p, "start", SANS, word);
        x += width + gap;
    }
    let sub = ease_out(seg(t, 5.4, 6.0));
    out += &text(W / 2., H / 2. + 130., 34., 400, "#FFFFFF", alpha * sub * 0.6, "middle", SANS, "Dictation that runs on your Mac's Neural Engine.");
    out
}

/// The Mac screen with a chat app, the notch indicator and the keycap.
fn scene_demo(t: f32) -> String {
    let alpha = scene_alpha(t, DEMO);
    let enter = ease_out(seg(t, DEMO.0, DEMO.0 + 0.8));
    let (sw, sh) = (1500., 860.);
    let (sx, sy) = ((W - sw) / 2., 70. + (1. - enter) * 60.);
    let mut out = format!(r#"<g opacity="{alpha:.3}">"#);

    // Screen and wallpaper.
    out += &format!(
        r##"<rect x="{sx}" y="{sy}" width="{sw}" height="{sh}" rx="26" fill="url(#wall)"/>
<rect x="{sx}" y="{sy}" width="{sw}" height="{sh}" rx="26" fill="none" stroke="#FFFFFF" stroke-opacity="0.12" stroke-width="2"/>"##
    );
    // Menu bar.
    out += &format!(r##"<rect x="{sx}" y="{sy}" width="{sw}" height="38" rx="0" fill="#000" fill-opacity="0.35"/>"##);
    out += &text(sx + 34., sy + 26., 17., 700, "#FFFFFF", 0.9, "start", SANS, "Slack");
    for (i, item) in ["File", "Edit", "View", "Go", "Window"].iter().enumerate() {
        out += &text(sx + 110. + i as f32 * 62., sy + 26., 17., 400, "#FFFFFF", 0.75, "start", SANS, item);
    }
    out += &text(sx + sw - 30., sy + 26., 17., 400, "#FFFFFF", 0.8, "end", SANS, "Tue 9:41");

    // Chat window.
    let (cx, cy, cw, ch) = (sx + 230., sy + 90., sw - 460., sh - 150.);
    out += &format!(
        r##"<rect x="{cx}" y="{cy}" width="{cw}" height="{ch}" rx="16" fill="#1C1C22" stroke="#FFFFFF" stroke-opacity="0.08"/>
<rect x="{cx}" y="{cy}" width="{cw}" height="56" rx="16" fill="#24242B"/>
<rect x="{cx}" y="{:.1}" width="{cw}" height="16" fill="#24242B"/>"##,
        cy + 40.
    );
    out += &text(cx + 28., cy + 36., 20., 700, "#FFFFFF", 0.95, "start", SANS, "# launch");
    let messages = [
        ("Priya", "Is the dictation build ready for testing?", "#F2994A"),
        ("Sam", "Waiting on the latency numbers.", "#56CCF2"),
    ];
    for (i, (who, msg, color)) in messages.iter().enumerate() {
        let y = cy + 120. + i as f32 * 96.;
        out += &format!(r#"<circle cx="{:.1}" cy="{:.1}" r="20" fill="{color}"/>"#, cx + 50., y - 6.);
        out += &text(cx + 84., y - 10., 18., 700, "#FFFFFF", 0.95, "start", SANS, who);
        out += &text(cx + 84., y + 18., 19., 400, "#FFFFFF", 0.75, "start", SANS, msg);
    }

    // Message field, filled after release.
    let (fx, fy, fw, fh) = (cx + 24., cy + ch - 86., cw - 48., 60.);
    out += &format!(
        r##"<rect x="{fx}" y="{fy}" width="{fw}" height="{fh}" rx="12" fill="#121216" stroke="#FFFFFF" stroke-opacity="{:.2}"/>"##,
        if t > KEY_UP { 0.35 } else { 0.14 }
    );
    let typed = seg(t, KEY_UP + 0.12, KEY_UP + 0.42);
    let chars = (TRANSCRIPT.chars().count() as f32 * typed) as usize;
    let field_text: String = TRANSCRIPT.chars().take(chars).collect();
    if chars == 0 {
        out += &text(fx + 22., fy + 38., 19., 400, "#FFFFFF", 0.35, "start", SANS, "Message #launch");
    } else {
        out += &text(fx + 22., fy + 38., 19., 400, "#FFFFFF", 0.95, "start", SANS, &field_text);
    }
    // Caret blink.
    if (t * 2.).fract() < 0.55 && t < KEY_DOWN {
        out += &format!(r#"<rect x="{:.1}" y="{:.1}" width="2" height="26" fill="{ACCENT}"/>"#, fx + 22., fy + 17.);
    }

    // Timing badge.
    let badge = ease_out(seg(t, KEY_UP + 0.5, KEY_UP + 0.9)) * (1. - seg(t, DEMO.1 - 0.6, DEMO.1));
    if badge > 0. {
        let (bx, by) = (fx + fw - 180., fy - 52.);
        out += &format!(
            r##"<g opacity="{badge:.3}"><rect x="{bx}" y="{by}" width="180" height="38" rx="19" fill="{ACCENT}" fill-opacity="0.18" stroke="{ACCENT}" stroke-opacity="0.6"/>"##
        );
        out += &text(bx + 90., by + 25., 17., 700, "#7FBFFF", 1., "middle", SANS, "typed in 72 ms");
        out += "</g>";
    }

    out += &notch(t, sx + sw / 2., sy);
    out += &keycap(t, W / 2., sy + sh + 72.);

    // Caption.
    let (cap, cap_alpha) = if t < KEY_UP {
        ("Hold the key and talk. Your words appear as you speak.", seg(t, KEY_DOWN - 0.3, KEY_DOWN + 0.3))
    } else {
        ("Let go — it's typed where your cursor is.", seg(t, KEY_UP, KEY_UP + 0.4))
    };
    out += &text(W / 2. + 120., sy + sh + 82., 26., 400, "#FFFFFF", cap_alpha * 0.85, "start", SANS, cap);
    out += "</g>";
    out
}

/// The black tab hanging from the notch.
fn notch(t: f32, cx: f32, top: f32) -> String {
    let (nw, nh) = (200., 38.);
    let open = spring(t - KEY_DOWN) * (1. - spring(t - KEY_UP));
    let open = if t < KEY_DOWN { 0. } else { open.max(0.) };
    let words: Vec<&str> = TRANSCRIPT.split(' ').collect();
    let speaking = seg(t, KEY_DOWN + 0.5, KEY_UP - 0.2);
    let shown = ((words.len() as f32) * speaking).ceil() as usize;
    let live: String = words[..shown.min(words.len())].join(" ");
    // Only the tail fits under the notch.
    let max_chars = 62;
    let live = if live.chars().count() > max_chars {
        let tail: String = live.chars().skip(live.chars().count() - max_chars + 1).collect();
        format!("…{}", tail.split_once(' ').map(|(_, r)| r).unwrap_or(&tail))
    } else {
        live
    };
    let row = if shown > 0 && t < KEY_UP { ease_out(seg(t, KEY_DOWN + 0.5, KEY_DOWN + 0.9)) } else { 0. } * open.min(1.);
    let text_w = (live.chars().count() as f32 * 11.5 + 70.).max(nw + 140.);
    let w = lerp(nw, (nw + 140.).max(lerp(nw + 140., text_w, row)), clamp01(open));
    let h = nh + 46. * row;
    let r = 14. + 8. * row;
    let x = cx - w / 2.;
    let mut out = format!(
        r##"<path d="M{x:.1},{top:.1} h{w:.1} v{:.1} a{r:.1},{r:.1} 0 0 1 -{r:.1},{r:.1} h-{:.1} a{r:.1},{r:.1} 0 0 1 -{r:.1},-{r:.1} z" fill="#000"/>"##,
        h - r,
        w - 2. * r
    );
    let wings = clamp01((open - 0.4) / 0.6);
    if wings > 0. && t < KEY_UP + 0.25 {
        // Recording dot.
        let pulse = 0.6 + 0.4 * (t * TAU * 0.9).cos().abs();
        out += &format!(
            r#"<circle cx="{:.1}" cy="{:.1}" r="7" fill="{RED}" opacity="{:.3}"/>"#,
            x + 34.,
            top + nh / 2.,
            wings * pulse
        );
        // Voice bars.
        for i in 0..5 {
            let level = 0.35 + 0.65 * ((t * 9. + i as f32 * 1.3).sin() * 0.5 + 0.5) * (1. - (i as f32 - 2.).abs() * 0.18);
            let bh = 6. + 18. * level * speaking.min(1.).max(0.3);
            out += &format!(
                r##"<rect x="{:.1}" y="{:.1}" width="4" height="{bh:.1}" rx="2" fill="#FFF" opacity="{wings:.3}"/>"##,
                x + w - 64. + i as f32 * 8.,
                top + nh / 2. - bh / 2.
            );
        }
    }
    if row > 0.01 {
        out += &text(cx, top + nh + 30., 19., 500, "#FFFFFF", row * 0.95, "middle", SANS, &live);
    }
    out
}

fn keycap(t: f32, cx: f32, cy: f32) -> String {
    let appear = ease_out(seg(t, KEY_DOWN - 0.8, KEY_DOWN - 0.3)) * (1. - seg(t, DEMO.1 - 0.6, DEMO.1));
    let pressed = t >= KEY_DOWN && t < KEY_UP;
    let press = if pressed { ease_out(seg(t, KEY_DOWN, KEY_DOWN + 0.12)) } else { 1. - ease_out(seg(t, KEY_UP, KEY_UP + 0.12)) };
    let (w, h) = (190., 84.);
    let x = cx - 250. - w / 2.;
    let y = cy - h / 2. + press * 5.;
    format!(
        r##"<g opacity="{appear:.3}">
<rect x="{x:.1}" y="{:.1}" width="{w}" height="{h}" rx="16" fill="#000" fill-opacity="0.5"/>
<rect x="{x:.1}" y="{y:.1}" width="{w}" height="{h}" rx="16" fill="{}" stroke="#FFFFFF" stroke-opacity="0.22"/>
{}</g>"##,
        cy - h / 2. + 7.,
        if pressed { "#2A3A55" } else { "#2A2A31" },
        text(x + w / 2., y + 52., 30., 600, if pressed { "#7FBFFF" } else { "#FFFFFF" }, 1., "middle", SANS, "Right ⌘"),
    )
}

fn scene_stats(t: f32) -> String {
    let alpha = scene_alpha(t, STATS);
    let stats = [
        (100., "×", "faster than real time", "~"),
        (0., "", "bytes of audio uploaded", ""),
        (6., "", "on-device speech models", ""),
    ];
    let mut out = String::new();
    out += &text(W / 2., 300., 46., 700, "#FFFFFF", alpha * ease_out(seg(t, STATS.0, STATS.0 + 0.6)), "middle", SANS, "Fast, private, yours.");
    for (i, (value, unit, label, prefix)) in stats.iter().enumerate() {
        let start = STATS.0 + 0.4 + i as f32 * 0.35;
        let p = ease_out(seg(t, start, start + 1.1));
        let x = W / 2. + (i as f32 - 1.) * 520.;
        let shown = (value * p).round() as u32;
        let rise = (1. - p) * 30.;
        out += &text(x, 560. + rise, 150., 700, if i == 0 { ACCENT } else { "#FFFFFF" }, alpha * clamp01(p * 2.), "middle", SANS, &format!("{prefix}{shown}{unit}"));
        out += &text(x, 640. + rise, 30., 400, "#FFFFFF", alpha * clamp01(p * 2.) * 0.65, "middle", SANS, label);
    }
    out
}

fn scene_models(t: f32) -> String {
    let alpha = scene_alpha(t, MODELS);
    let models = [
        ("Parakeet v3", "25 languages"),
        ("Parakeet v2", "English"),
        ("Bark 3.0", "205 MB"),
        ("Bark 3.0 Plus", "English"),
        ("Moonshine", "58 MB"),
    ];
    let mut out = text(W / 2., 360., 46., 700, "#FFFFFF", alpha * ease_out(seg(t, MODELS.0, MODELS.0 + 0.6)), "middle", SANS, "Pick your model. It never leaves your Mac.");
    let (cw, gap) = (300., 28.);
    let total = models.len() as f32 * cw + (models.len() - 1) as f32 * gap;
    for (i, (name, detail)) in models.iter().enumerate() {
        let start = MODELS.0 + 0.35 + i as f32 * 0.16;
        let p = spring(t - start).min(1.15);
        let x = (W - total) / 2. + i as f32 * (cw + gap);
        let y = 480. + (1. - p) * 80.;
        let o = alpha * clamp01((t - start) * 3.);
        out += &format!(
            r##"<g opacity="{o:.3}"><rect x="{x:.1}" y="{y:.1}" width="{cw}" height="150" rx="22" fill="#1C1C22" stroke="#FFFFFF" stroke-opacity="0.12"/>"##
        );
        out += &text(x + 28., y + 62., 32., 700, "#FFFFFF", 1., "start", SANS, name);
        out += &text(x + 28., y + 104., 22., 400, "#FFFFFF", 0.6, "start", SANS, detail);
        out += &format!(r#"<circle cx="{:.1}" cy="{:.1}" r="6" fill="{}"/>"#, x + cw - 34., y + 54., if i == 4 { "#8E8E93" } else { ACCENT });
        out += "</g>";
    }
    out += &text(W / 2., 720., 24., 400, "#FFFFFF", alpha * 0.55 * ease_out(seg(t, MODELS.0 + 1.3, MODELS.0 + 1.9)), "middle", SANS, "Neural Engine models via CoreML · CPU models via ONNX Runtime");
    out
}

fn scene_end(t: f32, a: &Assets) -> String {
    let alpha = scene_alpha(t, END);
    let s = spring(t - END.0 - 0.1);
    let line = |at: f32| ease_out(seg(t, END.0 + at, END.0 + at + 0.6));
    let cmd = "brew install --cask hiteshbandhu/tap/zappp";
    let (bw, bh) = (980., 86.);
    let (bx, by) = ((W - bw) / 2., 640. + (1. - line(0.9)) * 20.);
    format!(
        r##"{}{}{}{}<g opacity="{:.3}"><rect x="{bx:.1}" y="{by:.1}" width="{bw}" height="{bh}" rx="18" fill="#16161C" stroke="#FFFFFF" stroke-opacity="0.16"/>{}{}</g>{}"##,
        glow(W / 2., 330., 420., alpha * 0.8 * clamp01(s)),
        icon(a, W / 2., 320., 200. * lerp(0.7, 1., s), alpha * clamp01(s * 1.5)),
        text(W / 2., 500., 88., 700, "#FFFFFF", alpha * line(0.3), "middle", SANS, "zappp"),
        text(W / 2., 566., 30., 400, "#FFFFFF", alpha * line(0.55) * 0.7, "middle", SANS, "Free and open source · Apple Silicon · macOS 14+"),
        alpha * line(0.9),
        text(bx + 40., by + 56., 32., 400, "#8E8E93", 1., "start", MONO, "$"),
        text(bx + 80., by + 56., 32., 400, "#FFFFFF", 1., "start", MONO, cmd),
        text(W / 2., 820., 28., 500, "#7FBFFF", alpha * line(1.3), "middle", SANS, "github.com/hiteshbandhu/zappp"),
    )
}

// ── Frame assembly ──────────────────────────────────────────────────────

fn frame(t: f32, assets: &Assets) -> String {
    let mut body = String::new();
    let scenes: [(&(f32, f32), &dyn Fn() -> String); 6] = [
        (&LOGO, &|| scene_logo(t, assets)),
        (&TAGLINE, &|| scene_tagline(t)),
        (&DEMO, &|| scene_demo(t)),
        (&STATS, &|| scene_stats(t)),
        (&MODELS, &|| scene_models(t)),
        (&END, &|| scene_end(t, assets)),
    ];
    for (window, draw) in scenes {
        if t >= window.0 && t <= window.1 {
            body += &draw();
        }
    }
    // Fade from and to black at the very ends.
    let edge = seg(t, 0., 0.4).min(1. - seg(t, DURATION - 0.6, DURATION));
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{W}" height="{H}" viewBox="0 0 {W} {H}">
<defs>
  <linearGradient id="bg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="{BG_TOP}"/><stop offset="1" stop-color="{BG_BOTTOM}"/></linearGradient>
  <radialGradient id="glow"><stop offset="0" stop-color="{ACCENT}" stop-opacity="0.35"/><stop offset="1" stop-color="{ACCENT}" stop-opacity="0"/></radialGradient>
  <linearGradient id="wall" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#1D2B4F"/><stop offset="0.55" stop-color="#2B1F45"/><stop offset="1" stop-color="#0F1626"/></linearGradient>
</defs>
<rect width="{W}" height="{H}" fill="url(#bg)"/>
<g opacity="{edge:.3}">{body}</g>
</svg>"##
    )
}

fn render(svg: &str, opt: &usvg::Options) -> tiny_skia::Pixmap {
    let tree = usvg::Tree::from_str(svg, opt).expect("valid frame svg");
    let mut pixmap = tiny_skia::Pixmap::new(W as u32, H as u32).expect("pixmap");
    resvg::render(&tree, tiny_skia::Transform::identity(), &mut pixmap.as_mut());
    pixmap
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len() * 4 / 3 + 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let icon_png = std::fs::read(root.join("brand/icon-1024.png")).expect("brand/icon-1024.png");
    let assets = Assets {
        icon_uri: format!("data:image/png;base64,{}", base64(&icon_png)),
    };
    let mut opt = usvg::Options::default();
    opt.fontdb_mut().load_system_fonts();
    let opt = usvg::Options {
        fontdb: Arc::clone(&opt.fontdb),
        ..usvg::Options::default()
    };

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--still") {
        let t: f32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(9.5);
        let out = args.get(2).map(String::as_str).unwrap_or("frame.png");
        render(&frame(t, &assets), &opt).save_png(out).expect("write png");
        println!("wrote {out} (t = {t}s)");
        return;
    }

    let out = args.first().map(String::as_str).unwrap_or("dist/zappp-launch.mp4");
    let mut ffmpeg = Command::new("ffmpeg")
        .args(["-loglevel", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgba"])
        .args(["-s", &format!("{}x{}", W as u32, H as u32), "-r", &FPS.to_string(), "-i", "-"])
        .args(["-c:v", "libx264", "-preset", "slow", "-crf", "18", "-pix_fmt", "yuv420p", "-movflags", "+faststart"])
        .arg(out)
        .stdin(Stdio::piped())
        .spawn()
        .expect("ffmpeg must be installed");
    let mut stdin = ffmpeg.stdin.take().unwrap();
    let frames = (DURATION * FPS) as usize;
    for i in 0..frames {
        let pixmap = render(&frame(i as f32 / FPS, &assets), &opt);
        stdin.write_all(pixmap.data()).expect("ffmpeg closed its input");
        if i % 60 == 0 {
            eprint!("\rrendering {}/{frames}", i);
        }
    }
    drop(stdin);
    let status = ffmpeg.wait().expect("ffmpeg");
    eprintln!("\rrendered {frames} frames → {out} ({status})");
}
