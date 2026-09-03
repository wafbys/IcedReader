//! 实际渲染字体，经 Chromium DevTools Protocol 读取（同 DevTools Rendered Fonts，仅 Windows WebView2）。

use std::sync::mpsc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::Manager;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformFontUsage {
    pub family_name: String,
    pub glyph_count: u64,
    pub is_custom_font: bool,
}

pub async fn collect(app: tauri::AppHandle) -> Result<Vec<PlatformFontUsage>, String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "missing main window".to_string())?;
    let (tx, rx) = mpsc::channel();
    window
        .with_webview(move |webview| {
            windows_impl::query(webview, tx);
        })
        .map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        rx.recv_timeout(Duration::from_secs(8))
            .map_err(|_| "读取实际字体超时".to_string())?
    })
    .await
    .map_err(|e| e.to_string())?
}

mod windows_impl {
    use super::PlatformFontUsage;
    use std::sync::mpsc::Sender;

    use serde::Deserialize;
    use serde_json::Value;
    use tauri::webview::PlatformWebview;
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2;
    use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
    use windows::core::HSTRING;

    pub fn query(webview: PlatformWebview, tx: Sender<Result<Vec<PlatformFontUsage>, String>>) {
        let _ = tx.send(query_inner(webview));
    }

    fn query_inner(webview: PlatformWebview) -> Result<Vec<PlatformFontUsage>, String> {
        let core = unsafe { webview.controller().CoreWebView2().map_err(|e| e.to_string())? };
        cdp_sync(&core, "DOM.enable", "{}")?;
        cdp_sync(&core, "CSS.enable", "{}")?;
        let _ = cdp_sync(&core, "Runtime.enable", "{}");
        let _ = cdp_sync(&core, "DOM.getDocument", r#"{"depth":0}"#);

        let object_id = eval_object_id(
            &core,
            "(() => { const h = document.querySelector('#iced-font-probe'); if (!h) return null; const root = h.shadowRoot; return (root && root.querySelector('.iced-body')) || h; })()",
        )?;
        let node_id = node_id_from_object(&core, &object_id)?;
        let _ = cdp_sync(
            &core,
            "CSS.getComputedStyleForNode",
            &format!(r#"{{"nodeId":{node_id}}}"#),
        );
        let fonts = platform_fonts_for(&core, node_id)?;
        if fonts.is_empty() {
            return Err("探测副本已排版，但引擎未统计到用字".into());
        }
        Ok(fonts)
    }

    fn eval_object_id(core: &ICoreWebView2, expression: &str) -> Result<String, String> {
        let params = serde_json::json!({
            "expression": expression,
            "returnByValue": false,
            "awaitPromise": false,
        })
        .to_string();
        let json = cdp_sync(core, "Runtime.evaluate", &params)?;
        let parsed: Value = serde_json::from_str(&json).map_err(|e| e.to_string())?;
        if parsed.get("exceptionDetails").is_some() {
            return Err(format!("读取章节 DOM 失败: {json}"));
        }
        parsed
            .pointer("/result/objectId")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| format!("章节 body 不可见: {json}"))
    }

    fn node_id_from_object(core: &ICoreWebView2, object_id: &str) -> Result<i64, String> {
        let req = serde_json::json!({ "objectId": object_id }).to_string();
        let json = cdp_sync(core, "DOM.requestNode", &req)?;
        if let Some(id) = json_i64(&json, &["nodeId"]).filter(|id| *id > 0) {
            return Ok(id);
        }
        let desc = cdp_sync(
            core,
            "DOM.describeNode",
            &serde_json::json!({ "objectId": object_id, "pierce": true, "depth": 0 }).to_string(),
        )?;
        let parsed: Value = serde_json::from_str(&desc).map_err(|e| e.to_string())?;
        if let Some(id) = parsed
            .pointer("/node/nodeId")
            .and_then(Value::as_i64)
            .filter(|id| *id > 0)
        {
            return Ok(id);
        }
        let backend = parsed
            .pointer("/node/backendNodeId")
            .and_then(Value::as_i64)
            .ok_or_else(|| format!("describeNode 无 backendNodeId: {desc}"))?;
        let pushed = cdp_sync(
            core,
            "DOM.pushNodesByBackendIdsToFrontend",
            &format!(r#"{{"backendNodeIds":[{backend}]}}"#),
        )?;
        json_first_i64(&pushed, "nodeIds")
            .filter(|id| *id > 0)
            .ok_or_else(|| format!("无法把章节节点推入文档树: {pushed}"))
    }

    fn json_first_i64(json: &str, key: &str) -> Option<i64> {
        let v: Value = serde_json::from_str(json).ok()?;
        v.get(key)?.as_array()?.first()?.as_i64()
    }

    fn platform_fonts_for(
        core: &ICoreWebView2,
        node_id: i64,
    ) -> Result<Vec<PlatformFontUsage>, String> {
        let json = cdp_sync(
            core,
            "CSS.getPlatformFontsForNode",
            &format!(r#"{{"nodeId":{node_id}}}"#),
        )?;
        parse_fonts(&json)
    }

    fn cdp_sync(core: &ICoreWebView2, method: &str, params: &str) -> Result<String, String> {
        let (tx, rx) = std::sync::mpsc::channel();
        let core = core.clone();
        let method_s = method.to_string();
        let params_s = params.to_string();
        let method_err = method.to_string();
        CallDevToolsProtocolMethodCompletedHandler::wait_for_async_operation(
            Box::new(move |handler| {
                let method = HSTRING::from(method_s.as_str());
                let params = HSTRING::from(params_s.as_str());
                unsafe { core.CallDevToolsProtocolMethod(&method, &params, &handler)? };
                Ok(())
            }),
            Box::new(move |status, json| {
                let _ = tx.send(match status {
                    Ok(()) => Ok(json),
                    Err(err) => Err(format!("{method_err}: {err}")),
                });
                Ok(())
            }),
        )
        .map_err(|e| format!("{method} 等待失败: {e}"))?;
        rx.recv().map_err(|e| format!("{method} 无结果: {e}"))?
    }

    fn json_i64(json: &str, path: &[&str]) -> Option<i64> {
        let mut cur = serde_json::from_str::<Value>(json).ok()?;
        for key in path {
            cur = cur.get(*key)?.clone();
        }
        cur.as_i64()
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct CdpFonts {
        fonts: Vec<CdpFont>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct CdpFont {
        family_name: String,
        glyph_count: f64,
        #[serde(default)]
        is_custom_font: bool,
    }

    fn parse_fonts(json: &str) -> Result<Vec<PlatformFontUsage>, String> {
        let parsed: CdpFonts =
            serde_json::from_str(json).map_err(|e| format!("解析用字失败: {e}; {json}"))?;
        Ok(parsed
            .fonts
            .into_iter()
            .filter(|f| f.glyph_count > 0.0 && !f.family_name.is_empty())
            .map(|f| PlatformFontUsage {
                family_name: f.family_name,
                glyph_count: f.glyph_count.max(0.0) as u64,
                is_custom_font: f.is_custom_font,
            })
            .collect())
    }
}
