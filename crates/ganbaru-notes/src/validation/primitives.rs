pub fn validate_sort_order(value: f64) -> Result<(), String> {
    if value.is_finite() && value >= 0.0 {
        Ok(())
    } else {
        Err("sort_order must be non-negative".to_string())
    }
}

pub fn validate_page_size(page_size: i64) -> Result<(), String> {
    if (1..=100).contains(&page_size) {
        Ok(())
    } else {
        Err("page_size must be between 1 and 100".to_string())
    }
}

pub fn validate_children_count(count: usize) -> Result<(), String> {
    if (1..=100).contains(&count) {
        Ok(())
    } else {
        Err("children must include between 1 and 100 blocks".to_string())
    }
}

pub fn validate_duplicate_block_count(count: usize) -> Result<(), String> {
    if (1..=100).contains(&count) {
        Ok(())
    } else {
        Err("duplicated_block_ids must include between 1 and 100 blocks".to_string())
    }
}
