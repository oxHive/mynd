pub mod agent;

/// Extra system-prompt sentence for a bot whose room or channel maps to a
/// project, so the memories it stores get a home in that project's index
/// tree instead of becoming orphans. `None` without a `project:*` tag.
pub fn filing_hint(tags: &[String]) -> Option<String> {
    let project = tags.iter().find_map(|t| {
        t.to_lowercase()
            .strip_prefix("project:")
            .map(str::to_string)
    })?;
    Some(format!(
        " Before storing a new memory, find the project root with memory_search tags \
         [\"role:root\", \"project:{project}\"], pick the index pointer that best fits \
         (or the root itself), and pass its id as parent_id plus a one-line pointer \
         saying what the memory contains and when to open it."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filing_hint_names_the_project_root_search() {
        let h = filing_hint(&["topic:x".to_string(), "Project:Mynd".to_string()]).unwrap();
        assert!(h.contains("\"project:mynd\""), "{h}");
        assert!(h.contains("parent_id"), "{h}");
    }

    #[test]
    fn filing_hint_is_absent_without_a_project_tag() {
        assert!(filing_hint(&["topic:x".to_string()]).is_none());
    }
}
