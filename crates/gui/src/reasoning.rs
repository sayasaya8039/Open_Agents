//! 高度な推論モード: ReAct, Tree-of-Thoughts, Self-Consistency
//!
//! main.rs から分離した推論ループ・ツール実行・多数決の自由関数群。

use crate::chat_client;
use crate::chat_session::{ChatMsg, ChatMsgMetrics};
use crate::llama_cpp_chat;
use crate::model_prefs;

use super::ChatStreamEvent;

// ── ReAct (Reasoning + Acting) ループ実行 ──

/// ReAct のシステムプロンプト — ツール呼び出し形式を LLM に教える
pub(crate) const REACT_SYSTEM_SUFFIX: &str = r#"

## ReAct ツール使用ルール

あなたは以下のツールを使えます。ツールを使いたい場合は、必ず以下の形式で出力してください：

Thought: [今何を考えているか、次に何をすべきか]
Action: [ツール名]
Action Input: [ツールへの入力]

ツールの結果は Observation: として返されます。それを見て次の Thought を続けてください。
最終的な回答が得られたら、以下の形式で終了してください：

Thought: 十分な情報が得られたので最終回答をまとめます。
Final Answer: [最終回答]

### 利用可能なツール

1. **web_search** — ウェブ検索。キーワードを入力すると検索結果を返します。
   例: Action: web_search / Action Input: Rust async runtime comparison 2024

2. **calculate** — 数式を計算します。四則演算、累乗、括弧が使えます。
   例: Action: calculate / Action Input: (1024 * 1024 * 8) / 1000000

3. **datetime** — 現在の日付と時刻を返します。入力不要。
   例: Action: datetime / Action Input: now

重要: Action を使わず直接回答できる場合は Final Answer: で回答してください。"#;

/// ReAct ループを実行
pub(crate) fn run_react_loop(
    path: &std::path::Path,
    api_messages: &[(String, String)],
    temperature: f32,
    max_tokens: i32,
    context_length: i32,
    hardware: &model_prefs::HardwareParams,
    max_steps: usize,
    tx: &smol::channel::Sender<ChatStreamEvent>,
) {
    let started = std::time::Instant::now();

    // システムメッセージに ReAct 指示を追加
    let mut messages: Vec<(String, String)> = api_messages.to_vec();
    if let Some((_, sys)) = messages.iter_mut().find(|(r, _)| r == "system") {
        sys.push_str(REACT_SYSTEM_SUFFIX);
    }

    let _ = tx.send_blocking(ChatStreamEvent::ContentDelta(
        "🔄 **ReAct**: Reasoning + Acting ループを開始…\n\n".into(),
    ));

    let mut trace = String::new(); // thinking に格納する全トレース
    let mut step = 0;

    loop {
        if step >= max_steps {
            let _ = tx.send_blocking(ChatStreamEvent::ContentDelta(format!(
                "\n⚠ 最大ステップ数 ({max_steps}) に到達。最終回答を強制生成します…\n\n"
            )));
            // 最終回答を強制
            messages.push((
                "user".into(),
                "最大ステップ数に達しました。これまでの情報をもとに Final Answer: を出力してください。".into(),
            ));
            break;
        }

        // LLM 呼び出し
        let result = llama_cpp_chat::complete_llama_cpp_chat_blocking(
            path,
            &messages,
            temperature,
            max_tokens,
            context_length,
            hardware,
        );

        let reply_text = match result {
            Ok(reply) => reply.content,
            Err(e) => {
                let _ = tx.send_blocking(ChatStreamEvent::Error(format!(
                    "ReAct ステップ {} 失敗: {e}",
                    step + 1
                )));
                return;
            }
        };

        trace.push_str(&format!("--- Step {} ---\n{}\n\n", step + 1, reply_text));

        // Final Answer を検出
        if let Some(final_answer) = extract_final_answer(&reply_text) {
            let elapsed_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
            let mut metrics = ChatMsgMetrics::default();
            metrics.elapsed_ms = Some(elapsed_ms);
            metrics.stop_reason = Some(format!("ReAct {} ステップ", step + 1));

            let _ = tx.send_blocking(ChatStreamEvent::Complete(
                llama_cpp_chat::LlamaCppChatResponse {
                    content: format!(
                        "{final_answer}\n\n---\n*🔄 ReAct: {} ステップで到達*",
                        step + 1
                    ),
                    thinking: Some(trace),
                    metrics: Some(metrics),
                },
            ));
            return;
        }

        // Action を検出してツール実行
        if let Some((action, input)) = extract_action(&reply_text) {
            step += 1;
            let _ = tx.send_blocking(ChatStreamEvent::ContentDelta(format!(
                "**Step {step}**: `{action}({input})` を実行中…\n"
            )));

            let observation = execute_react_tool(&action, &input);

            let _ = tx.send_blocking(ChatStreamEvent::ContentDelta(format!(
                "→ 結果取得完了\n"
            )));

            trace.push_str(&format!("Observation: {}\n\n", observation));

            // LLM の出力 + Observation を会話に追加
            messages.push(("assistant".into(), reply_text));
            messages.push((
                "user".into(),
                format!("Observation: {observation}\n\n上記の結果を踏まえて、次の Thought を続けてください。最終回答が出せるなら Final Answer: で回答してください。"),
            ));
        } else {
            // Action も Final Answer もない → 直接回答として扱う
            let elapsed_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
            let mut metrics = ChatMsgMetrics::default();
            metrics.elapsed_ms = Some(elapsed_ms);
            metrics.stop_reason = Some("ReAct 直接回答".into());

            let _ = tx.send_blocking(ChatStreamEvent::Complete(
                llama_cpp_chat::LlamaCppChatResponse {
                    content: format!(
                        "{reply_text}\n\n---\n*🔄 ReAct: ツール不使用で直接回答*"
                    ),
                    thinking: if trace.is_empty() { None } else { Some(trace) },
                    metrics: Some(metrics),
                },
            ));
            return;
        }
    }

    // 最大ステップ後の最終回答
    let result = llama_cpp_chat::complete_llama_cpp_chat_blocking(
        path,
        &messages,
        temperature,
        max_tokens,
        context_length,
        hardware,
    );

    let elapsed_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    match result {
        Ok(reply) => {
            let content = extract_final_answer(&reply.content)
                .unwrap_or_else(|| reply.content.clone());
            let mut metrics = reply.metrics.unwrap_or_default();
            metrics.elapsed_ms = Some(elapsed_ms);
            metrics.stop_reason = Some(format!("ReAct {max_steps} ステップ上限"));

            let _ = tx.send_blocking(ChatStreamEvent::Complete(
                llama_cpp_chat::LlamaCppChatResponse {
                    content: format!(
                        "{content}\n\n---\n*🔄 ReAct: {max_steps} ステップで到達（上限）*"
                    ),
                    thinking: Some(trace),
                    metrics: Some(metrics),
                },
            ));
        }
        Err(e) => {
            let _ = tx.send_blocking(ChatStreamEvent::Error(format!(
                "ReAct 最終回答生成失敗: {e}"
            )));
        }
    }
}

/// LLM 出力から "Final Answer: ..." を抽出
pub(crate) fn extract_final_answer(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Final Answer:") {
            // Final Answer: 以降の全テキスト（複数行も含む）
            let pos = text.find("Final Answer:").unwrap();
            let answer = text[pos + "Final Answer:".len()..].trim();
            return Some(answer.to_string());
        }
    }
    None
}

/// LLM 出力から "Action: xxx / Action Input: yyy" を抽出
pub(crate) fn extract_action(text: &str) -> Option<(String, String)> {
    let mut action = None;
    let mut input = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Action:") {
            action = Some(rest.trim().to_string());
        }
        if let Some(rest) = trimmed.strip_prefix("Action Input:") {
            input = Some(rest.trim().to_string());
        }
    }
    match (action, input) {
        (Some(a), Some(i)) => Some((a, i)),
        (Some(a), None) => Some((a, String::new())),
        _ => None,
    }
}

/// ReAct ツールを実行して Observation を返す
pub(crate) fn execute_react_tool(action: &str, input: &str) -> String {
    let action_lower = action.trim().to_lowercase();
    match action_lower.as_str() {
        "web_search" | "search" => react_tool_web_search(input),
        "calculate" | "calc" | "math" => react_tool_calculate(input),
        "datetime" | "date" | "time" | "now" => react_tool_datetime(),
        _ => format!("未知のツール `{action}` です。利用可能: web_search, calculate, datetime"),
    }
}

/// ウェブ検索ツール — DuckDuckGo Lite HTML を取得してテキスト抽出
pub(crate) fn react_tool_web_search(query: &str) -> String {
    if query.trim().is_empty() {
        return "検索クエリが空です。".to_string();
    }
    let encoded = query
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c.to_string()
            } else if c == ' ' {
                "+".to_string()
            } else {
                format!("%{:02X}", c as u32)
            }
        })
        .collect::<String>();
    let url = format!("https://lite.duckduckgo.com/lite/?q={encoded}");
    match ureq::get(&url)
        .set("User-Agent", "Open Agents ReAct/1.0")
        .timeout(std::time::Duration::from_secs(10))
        .call()
    {
        Ok(resp) => {
            let text = resp.into_string().unwrap_or_default();
            // HTML からテキストを雑に抽出（結果スニペット部分）
            let snippets = extract_search_snippets(&text);
            if snippets.is_empty() {
                format!("検索結果が見つかりませんでした。クエリ: {query}")
            } else {
                snippets.join("\n\n")
            }
        }
        Err(e) => format!("検索失敗: {e}"),
    }
}

/// DuckDuckGo Lite HTML から検索結果スニペットを抽出（最大 5 件）
pub(crate) fn extract_search_snippets(html: &str) -> Vec<String> {
    let mut results = Vec::new();
    // DuckDuckGo Lite の結果は <td class="result-snippet"> に入る
    for segment in html.split("result-snippet") {
        if results.len() >= 5 {
            break;
        }
        if let Some(start) = segment.find('>') {
            let text_part = &segment[start + 1..];
            if let Some(end) = text_part.find("</td>").or(text_part.find("</span>")) {
                let raw = &text_part[..end];
                let cleaned = strip_html_tags(raw).trim().to_string();
                if cleaned.len() > 20 {
                    results.push(cleaned);
                }
            }
        }
    }
    // フォールバック: <a> タグのテキストも取得
    if results.is_empty() {
        for segment in html.split("result-link") {
            if results.len() >= 5 {
                break;
            }
            if let Some(start) = segment.find('>') {
                let text_part = &segment[start + 1..];
                if let Some(end) = text_part.find("</a>") {
                    let cleaned = strip_html_tags(&text_part[..end]).trim().to_string();
                    if cleaned.len() > 10 {
                        results.push(cleaned);
                    }
                }
            }
        }
    }
    results
}

/// HTML タグを雑に除去
pub(crate) fn strip_html_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        if c == '<' {
            in_tag = true;
        } else if c == '>' {
            in_tag = false;
        } else if !in_tag {
            out.push(c);
        }
    }
    // HTML エンティティの基本変換
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

/// 計算ツール — 簡易数式評価
pub(crate) fn react_tool_calculate(expr: &str) -> String {
    if expr.trim().is_empty() {
        return "数式が空です。".to_string();
    }
    match eval_simple_math(expr.trim()) {
        Some(result) => format!("{expr} = {result}"),
        None => format!("計算できませんでした: {expr} （四則演算・括弧・累乗のみ対応）"),
    }
}

/// 簡易数式パーサ（四則演算 + 括弧 + ** 累乗）
pub(crate) fn eval_simple_math(expr: &str) -> Option<f64> {
    let tokens = tokenize_math(expr)?;
    let mut pos = 0;
    let result = parse_expr(&tokens, &mut pos)?;
    if pos == tokens.len() {
        Some(result)
    } else {
        None
    }
}

#[derive(Debug, Clone)]
pub(crate) enum MathToken {
    Num(f64),
    Op(char),
    LParen,
    RParen,
}

pub(crate) fn tokenize_math(expr: &str) -> Option<Vec<MathToken>> {
    let mut tokens = Vec::new();
    let mut chars = expr.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else if c.is_ascii_digit() || c == '.' {
            let mut num_str = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_ascii_digit() || c == '.' {
                    num_str.push(c);
                    chars.next();
                } else {
                    break;
                }
            }
            tokens.push(MathToken::Num(num_str.parse().ok()?));
        } else if c == '(' {
            tokens.push(MathToken::LParen);
            chars.next();
        } else if c == ')' {
            tokens.push(MathToken::RParen);
            chars.next();
        } else if "+-*/%^".contains(c) {
            if c == '*' && chars.clone().nth(1) == Some('*') {
                chars.next();
                chars.next();
                tokens.push(MathToken::Op('^'));
            } else {
                tokens.push(MathToken::Op(c));
                chars.next();
            }
        } else {
            return None;
        }
    }
    Some(tokens)
}

pub(crate) fn parse_expr(tokens: &[MathToken], pos: &mut usize) -> Option<f64> {
    let mut left = parse_term(tokens, pos)?;
    while *pos < tokens.len() {
        match &tokens[*pos] {
            MathToken::Op('+') => { *pos += 1; left += parse_term(tokens, pos)?; }
            MathToken::Op('-') => { *pos += 1; left -= parse_term(tokens, pos)?; }
            _ => break,
        }
    }
    Some(left)
}

pub(crate) fn parse_term(tokens: &[MathToken], pos: &mut usize) -> Option<f64> {
    let mut left = parse_power(tokens, pos)?;
    while *pos < tokens.len() {
        match &tokens[*pos] {
            MathToken::Op('*') => { *pos += 1; left *= parse_power(tokens, pos)?; }
            MathToken::Op('/') => { *pos += 1; let r = parse_power(tokens, pos)?; left /= r; }
            MathToken::Op('%') => { *pos += 1; let r = parse_power(tokens, pos)?; left %= r; }
            _ => break,
        }
    }
    Some(left)
}

pub(crate) fn parse_power(tokens: &[MathToken], pos: &mut usize) -> Option<f64> {
    let base = parse_atom(tokens, pos)?;
    if *pos < tokens.len() && matches!(&tokens[*pos], MathToken::Op('^')) {
        *pos += 1;
        let exp = parse_power(tokens, pos)?;
        Some(base.powf(exp))
    } else {
        Some(base)
    }
}

pub(crate) fn parse_atom(tokens: &[MathToken], pos: &mut usize) -> Option<f64> {
    if *pos >= tokens.len() {
        return None;
    }
    match &tokens[*pos] {
        MathToken::Num(n) => { let v = *n; *pos += 1; Some(v) }
        MathToken::LParen => {
            *pos += 1;
            let v = parse_expr(tokens, pos)?;
            if *pos < tokens.len() && matches!(&tokens[*pos], MathToken::RParen) {
                *pos += 1;
            }
            Some(v)
        }
        MathToken::Op('-') => {
            *pos += 1;
            let v = parse_atom(tokens, pos)?;
            Some(-v)
        }
        _ => None,
    }
}

/// 日時ツール
pub(crate) fn react_tool_datetime() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    // UTC → JST (+9h)
    let jst_secs = secs + 9 * 3600;
    let days = jst_secs / 86400;
    let time_of_day = jst_secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // 簡易日付計算 (2000-01-01 = day 10957 from epoch)
    let (year, month, day) = days_to_date(days as i64);
    let weekday = ["木", "金", "土", "日", "月", "火", "水"][(days % 7) as usize];

    format!(
        "{year}年{month}月{day}日（{weekday}）{hours:02}:{minutes:02}:{seconds:02} JST"
    )
}

pub(crate) fn days_to_date(mut days: i64) -> (i64, u32, u32) {
    // Algorithm from Howard Hinnant
    days += 719468;
    let era = if days >= 0 { days } else { days - 146096 } / 146097;
    let doe = (days - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

// ── Tree-of-Thoughts (ToT) 3フェーズ実行 ──

/// ToT プロンプト: Phase 1 — 複数の思考経路を生成
pub(crate) fn tot_branch_prompt(user_question: &str, branch_count: usize) -> String {
    format!(
        "以下の質問について、{branch_count}つの異なるアプローチで考えてください。\
各アプローチは互いに独立した視点・手法で問題を解くものとします。\n\n\
質問: {user_question}\n\n\
各アプローチを「## アプローチ1」「## アプローチ2」…の形式で、\
それぞれステップバイステップの思考過程とともに出力してください。"
    )
}

/// ToT プロンプト: Phase 2 — 各経路を評価
pub(crate) fn tot_evaluate_prompt(branches_text: &str) -> String {
    format!(
        "以下の複数のアプローチを評価してください。\n\n\
{branches_text}\n\n\
各アプローチについて以下の観点で 1〜5 のスコアをつけ、最も優れたアプローチを1つ選んでください：\n\
- 正確性（論理的に正しいか）\n\
- 完全性（問題の全側面をカバーしているか）\n\
- 明快さ（説明がわかりやすいか）\n\n\
評価結果を出力した後、最後に「最優秀: アプローチN」と明記してください。"
    )
}

/// ToT プロンプト: Phase 3 — 最良経路を基に最終回答を合成
pub(crate) fn tot_synthesize_prompt(user_question: &str, branches_text: &str, evaluation: &str) -> String {
    format!(
        "以下の質問に対して複数のアプローチとその評価結果があります。\n\n\
質問: {user_question}\n\n\
=== アプローチ一覧 ===\n{branches_text}\n\n\
=== 評価結果 ===\n{evaluation}\n\n\
評価結果で最も高く評価されたアプローチを基に、他のアプローチの良い点も取り入れて、\
最終的な回答を作成してください。回答は明確で実用的なものにしてください。"
    )
}

/// ToT を 3 フェーズで実行し、進捗を tx に送信
pub(crate) fn run_tree_of_thoughts(
    path: &std::path::Path,
    api_messages: &[(String, String)],
    temperature: f32,
    max_tokens: i32,
    context_length: i32,
    hardware: &model_prefs::HardwareParams,
    branch_count: usize,
    tx: &smol::channel::Sender<ChatStreamEvent>,
) {
    // ユーザーの最後の質問を抽出
    let user_question = api_messages
        .iter()
        .rev()
        .find(|(role, _)| role == "user")
        .map(|(_, content)| content.as_str())
        .unwrap_or("（質問なし）");

    let system_msg = api_messages
        .iter()
        .find(|(role, _)| role == "system")
        .map(|(_, content)| content.as_str())
        .unwrap_or("");

    let started = std::time::Instant::now();

    // ── Phase 1: 分岐 — 複数の思考経路を生成 ──
    let _ = tx.send_blocking(ChatStreamEvent::ContentDelta(format!(
        "🌳 **Tree-of-Thoughts**: {branch_count} 経路を探索中…\n\n\
**Phase 1/3**: 思考経路を生成中…\n"
    )));

    let branch_request = tot_branch_prompt(user_question, branch_count);
    let branch_messages: Vec<(String, String)> = vec![
        ("system".into(), system_msg.to_string()),
        ("user".into(), branch_request),
    ];

    let branches_result = llama_cpp_chat::complete_llama_cpp_chat_blocking(
        path,
        &branch_messages,
        temperature,
        max_tokens,
        context_length,
        hardware,
    );

    let branches_text = match branches_result {
        Ok(reply) => {
            let _ = tx.send_blocking(ChatStreamEvent::ContentDelta(
                "✓ Phase 1 完了 — 思考経路を生成しました\n".into(),
            ));
            reply.content
        }
        Err(e) => {
            let _ = tx.send_blocking(ChatStreamEvent::Error(format!(
                "ToT Phase 1（分岐生成）失敗: {e}"
            )));
            return;
        }
    };

    // ── Phase 2: 評価 — 各経路をスコアリング ──
    let _ = tx.send_blocking(ChatStreamEvent::ContentDelta(
        "**Phase 2/3**: 各経路を評価中…\n".into(),
    ));

    let eval_request = tot_evaluate_prompt(&branches_text);
    let eval_messages: Vec<(String, String)> = vec![
        ("system".into(), system_msg.to_string()),
        ("user".into(), eval_request),
    ];

    let eval_result = llama_cpp_chat::complete_llama_cpp_chat_blocking(
        path,
        &eval_messages,
        (temperature * 0.5).max(0.1), // 評価は低温度で安定化
        max_tokens,
        context_length,
        hardware,
    );

    let evaluation = match eval_result {
        Ok(reply) => {
            let _ = tx.send_blocking(ChatStreamEvent::ContentDelta(
                "✓ Phase 2 完了 — 経路評価が終わりました\n".into(),
            ));
            reply.content
        }
        Err(e) => {
            // 評価失敗時は Phase 1 の結果をそのまま使用
            let _ = tx.send_blocking(ChatStreamEvent::ContentDelta(format!(
                "⚠ Phase 2 評価失敗 ({e})、Phase 1 の結果を直接使用します\n"
            )));
            format!("評価省略 — 最初のアプローチを採用")
        }
    };

    // ── Phase 3: 合成 — 最良経路から最終回答を生成 ──
    let _ = tx.send_blocking(ChatStreamEvent::ContentDelta(
        "**Phase 3/3**: 最終回答を合成中…\n\n---\n\n".into(),
    ));

    let synth_request = tot_synthesize_prompt(user_question, &branches_text, &evaluation);
    let synth_messages: Vec<(String, String)> = vec![
        ("system".into(), system_msg.to_string()),
        ("user".into(), synth_request),
    ];

    let synth_result = llama_cpp_chat::complete_llama_cpp_chat_blocking(
        path,
        &synth_messages,
        temperature,
        max_tokens,
        context_length,
        hardware,
    );

    let elapsed_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;

    match synth_result {
        Ok(reply) => {
            let mut metrics = reply.metrics.unwrap_or_default();
            metrics.elapsed_ms = Some(elapsed_ms);
            metrics.stop_reason = Some(format!("ToT {branch_count}経路探索"));

            let final_reply = llama_cpp_chat::LlamaCppChatResponse {
                content: format!(
                    "{}\n\n---\n*🌳 Tree-of-Thoughts: {branch_count} 経路を探索・評価・合成*",
                    reply.content
                ),
                thinking: Some(format!(
                    "=== Phase 1: 思考経路 ===\n{branches_text}\n\n=== Phase 2: 評価 ===\n{evaluation}"
                )),
                metrics: Some(metrics),
            };
            let _ = tx.send_blocking(ChatStreamEvent::Complete(final_reply));
        }
        Err(e) => {
            // Phase 3 失敗時は Phase 1 + Phase 2 の結果を返す
            let fallback = format!(
                "## 思考経路（ToT Phase 1）\n{branches_text}\n\n## 評価（ToT Phase 2）\n{evaluation}\n\n---\n*⚠ Phase 3（合成）失敗: {e}*"
            );
            let mut metrics = ChatMsgMetrics::default();
            metrics.elapsed_ms = Some(elapsed_ms);
            metrics.stop_reason = Some(format!("ToT fallback ({e})"));

            let _ = tx.send_blocking(ChatStreamEvent::Complete(
                llama_cpp_chat::LlamaCppChatResponse {
                    content: fallback,
                    thinking: None,
                    metrics: Some(metrics),
                },
            ));
        }
    }
}

/// Self-Consistency 多数決: 回答を正規化してグループ化し、最多得票を返す
/// 戻り値: (最良回答, メトリクス, 得票数, 総投票数)
pub(crate) fn majority_vote_select(
    responses: &[(String, Option<ChatMsgMetrics>)],
) -> (String, Option<ChatMsgMetrics>, usize, usize) {
    if responses.is_empty() {
        return (String::new(), None, 0, 0);
    }
    if responses.len() == 1 {
        return (
            responses[0].0.clone(),
            responses[0].1.clone(),
            1,
            1,
        );
    }

    // 各回答の「結論部分」を抽出して比較（最終段落 or 最後の文）
    let normalized: Vec<String> = responses
        .iter()
        .map(|(content, _)| normalize_for_vote(content))
        .collect();

    // 各回答ペアの類似度を計算し、最も他の回答と一致する回答を選択
    let mut best_idx = 0;
    let mut best_score = 0usize;
    for i in 0..normalized.len() {
        let mut score = 0;
        for j in 0..normalized.len() {
            if i != j && answers_are_similar(&normalized[i], &normalized[j]) {
                score += 1;
            }
        }
        if score > best_score {
            best_score = score;
            best_idx = i;
        }
    }

    (
        responses[best_idx].0.clone(),
        responses[best_idx].1.clone(),
        best_score + 1, // 自分自身を含む
        responses.len(),
    )
}

/// 回答を正規化: 空白・改行を統一、マークダウン装飾を除去して比較用テキストにする
pub(crate) fn normalize_for_vote(content: &str) -> String {
    // 最終結論を抽出: 「結論」「まとめ」「答え」等のキーワード以降を優先
    let conclusion_markers = ["## 結論", "## まとめ", "**結論", "**まとめ", "答え:", "結論:"];
    for marker in conclusion_markers {
        if let Some(pos) = content.to_lowercase().find(&marker.to_lowercase()) {
            let tail = &content[pos..];
            return normalize_text(tail);
        }
    }
    // 最終段落を使う（最後の2重改行以降）
    if let Some(pos) = content.rfind("\n\n") {
        let tail = content[pos..].trim();
        if tail.len() > 10 {
            return normalize_text(tail);
        }
    }
    normalize_text(content)
}

pub(crate) fn normalize_text(text: &str) -> String {
    text.chars()
        .filter(|c| !matches!(c, '*' | '#' | '`' | '>' | '-'))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// 2つの正規化済み回答が「同じ結論」かを判定（Jaccard 類似度 > 0.4）
pub(crate) fn answers_are_similar(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let words_a: std::collections::HashSet<&str> = a.split_whitespace().collect();
    let words_b: std::collections::HashSet<&str> = b.split_whitespace().collect();
    if words_a.is_empty() || words_b.is_empty() {
        return false;
    }
    let intersection = words_a.intersection(&words_b).count();
    let union = words_a.union(&words_b).count();
    if union == 0 {
        return false;
    }
    (intersection as f64 / union as f64) > 0.4
}

pub(crate) fn merge_metrics(
    existing: Option<ChatMsgMetrics>,
    incoming: Option<ChatMsgMetrics>,
) -> Option<ChatMsgMetrics> {
    incoming.or(existing)
}

pub(crate) fn apply_chat_completion_result(msg: &mut ChatMsg, response: chat_client::ChatCompletionResult) {
    msg.content = response.content;
    msg.metrics = merge_metrics(msg.metrics.take(), response.metrics);
}

pub(crate) fn apply_local_chat_response(msg: &mut ChatMsg, response: llama_cpp_chat::LlamaCppChatResponse) {
    msg.thinking = response.thinking;
    msg.metrics = merge_metrics(msg.metrics.take(), response.metrics);
    if response.content.is_empty() {
        if msg.thinking.is_some() {
            msg.content =
                "（思考トークンのみ受信しました。最大トークン数を増やすと回答本文まで届く場合があります）"
                    .into();
        } else {
            msg.content.clear();
        }
    } else {
        msg.content = response.content;
    }
}

pub(crate) fn apply_local_stream_completion(
    msg: &mut ChatMsg,
    response: llama_cpp_chat::LlamaCppChatResponse,
    saw_content_delta: bool,
    saw_thinking_delta: bool,
) {
    let llama_cpp_chat::LlamaCppChatResponse {
        content,
        thinking,
        metrics,
    } = response;

    msg.metrics = merge_metrics(msg.metrics.take(), metrics);

    if !saw_thinking_delta {
        msg.thinking = thinking;
    }

    if !saw_content_delta {
        if content.is_empty() {
            if msg.thinking.is_some() {
                msg.content =
                    "（思考トークンのみ受信しました。最大トークン数を増やすと回答本文まで届く場合があります）"
                        .into();
            } else {
                msg.content.clear();
            }
        } else {
            msg.content = content;
        }
    }
}
