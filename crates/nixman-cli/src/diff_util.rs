/// Simple line-by-line unified diff between two strings.
///
/// Outputs a minimal `--- a/…` / `+++ b/…` / `@@ … @@` format showing only
/// lines that differ.  Not a full unified diff — no surrounding context.
pub fn simple_diff(original: &str, modified: &str, filename: &str) -> String {
    let orig_lines: Vec<&str> = original.lines().collect();
    let mod_lines: Vec<&str> = modified.lines().collect();

    // Simple LCS-based diff: find the longest common subsequence of lines.
    // We only emit lines that differ.
    let mut output = String::new();
    output.push_str(&format!("--- a/{}\n", filename));
    output.push_str(&format!("+++ b/{}\n", filename));

    // Build LCS table
    let m = orig_lines.len();
    let n = mod_lines.len();
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 1..=m {
        for j in 1..=n {
            if orig_lines[i - 1] == mod_lines[j - 1] {
                dp[i][j] = dp[i - 1][j - 1] + 1;
            } else {
                dp[i][j] = dp[i - 1][j].max(dp[i][j - 1]);
            }
        }
    }

    // Backtrack through the LCS to collect diff hunks.
    let mut i = m;
    let mut j = n;
    let mut diff_lines: Vec<(usize, usize, char, &str)> = Vec::new(); // (orig_idx, mod_idx, '+',/'-'/line)

    // Walk backwards collecting changes
    let mut changes: Vec<(char, &str)> = Vec::new();
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && orig_lines[i - 1] == mod_lines[j - 1] {
            // Matching lines — flush any pending changes and emit context
            if !changes.is_empty() {
                changes.reverse();
                diff_lines.extend(changes.drain(..).map(|(c, l)| (i.saturating_sub(1), j.saturating_sub(1), c, l)));
            }
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
            changes.push(('+', mod_lines[j - 1]));
            j -= 1;
        } else {
            changes.push(('-', orig_lines[i - 1]));
            i -= 1;
        }
    }
    if !changes.is_empty() {
        changes.reverse();
        diff_lines.extend(changes.drain(..).map(|(c, l)| (i, j, c, l)));
    }

    // Emit hunks from collected diff_lines
    // Group changes into hunks of contiguous changed sections
    let mut idx = 0;
    while idx < diff_lines.len() {
        // Find a contiguous block of changes
        let mut end = idx;
        let mut prev_orig = diff_lines[idx].0;
        let mut prev_mod = diff_lines[idx].1;
        while end < diff_lines.len() {
            let (oi, mi, ..) = diff_lines[end];
            if end > idx {
                // Check if this is close to the previous change
                let orig_gap = if oi > prev_orig { oi - prev_orig } else { prev_orig - oi };
                let mod_gap = if mi > prev_mod { mi - prev_mod } else { prev_mod - mi };
                if orig_gap > 1 && mod_gap > 1 {
                    break;
                }
            }
            prev_orig = oi;
            prev_mod = mi;
            end += 1;
        }

        let hunk = &diff_lines[idx..end];
        if hunk.is_empty() {
            idx = end;
            continue;
        }

        // Calculate hunk header
        let hunk_count_orig = hunk.iter().filter(|(_, _, c, _)| *c == '-').count();
        let hunk_count_mod = hunk.iter().filter(|(_, _, c, _)| *c == '+').count();

        output.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            hunk[0].0 + 1,
            hunk_count_orig.max(1),
            hunk[0].1 + 1,
            hunk_count_mod.max(1),
        ));

        for (_, _, c, line) in hunk {
            output.push_str(&format!("{} {}\n", c, line));
        }

        idx = end;
    }

    output
}
