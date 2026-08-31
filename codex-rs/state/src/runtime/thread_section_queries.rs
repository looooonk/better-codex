use super::StateRuntime;

impl StateRuntime {
    /// Read an independently persisted thread section by its opaque identifier.
    pub async fn get_thread_section(
        &self,
        id: &str,
    ) -> anyhow::Result<Option<crate::ThreadSection>> {
        let row = sqlx::query_as::<_, (String, String, Option<String>)>(
            "SELECT id, name, appearance FROM thread_sections WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(self.pool.as_ref())
        .await?;
        row.map(crate::ThreadSection::from_row).transpose()
    }

    /// List independently persisted sections in stable, cursor-paginated identifier order.
    ///
    /// `limit` must be between one and [`crate::MAX_THREAD_SECTIONS_PAGE_SIZE`].
    pub async fn list_thread_sections(
        &self,
        cursor: Option<&str>,
        limit: usize,
    ) -> anyhow::Result<crate::ThreadSectionsPage> {
        if !(1..=crate::MAX_THREAD_SECTIONS_PAGE_SIZE).contains(&limit) {
            anyhow::bail!(
                "thread section page size must be between 1 and {}; got {limit}",
                crate::MAX_THREAD_SECTIONS_PAGE_SIZE
            );
        }
        let page_size = limit;
        let fetch_limit = i64::try_from(page_size.saturating_add(1))?;
        let rows = sqlx::query_as::<_, (String, String, Option<String>)>(
            r#"
SELECT id, name, appearance
FROM thread_sections
WHERE (? IS NULL OR id > ?)
ORDER BY id
LIMIT ?
            "#,
        )
        .bind(cursor)
        .bind(cursor)
        .bind(fetch_limit)
        .fetch_all(self.pool.as_ref())
        .await?;
        let mut sections = rows
            .into_iter()
            .map(crate::ThreadSection::from_row)
            .collect::<anyhow::Result<Vec<_>>>()?;
        let next_cursor = if sections.len() > page_size {
            sections.pop();
            sections.last().map(|section| section.id.clone())
        } else {
            None
        };
        Ok(crate::ThreadSectionsPage {
            sections,
            next_cursor,
        })
    }
}

#[cfg(test)]
#[path = "thread_sections_tests.rs"]
mod tests;
