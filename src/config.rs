//! 配置模块，负责加载JSON配置文件

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// 表映射配置错误
#[derive(Debug)]
pub struct ConfigError {
    pub message: String,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "配置错误: {}", self.message)
    }
}

impl std::error::Error for ConfigError {}

impl ConfigError {
    pub fn new(message: String) -> Self {
        Self { message }
    }
}

/// 表映射配置结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableMappingConfig {
    /// 实体名到数据库表名的映射
    #[serde(flatten)]
    pub mappings: HashMap<String, String>,
}

impl TableMappingConfig {
    /// 从JSON文件加载表映射配置
    pub fn from_json_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let path_ref = path.as_ref();
        
        // 检查文件是否存在
        if !path_ref.exists() {
            return Err(ConfigError::new(format!(
                "配置文件不存在: {}",
                path_ref.display()
            )));
        }
        
        // 读取文件内容
        let content = fs::read_to_string(path_ref)
            .map_err(|e| ConfigError::new(format!(
                "无法读取配置文件 {}: {}",
                path_ref.display(),
                e
            )))?;
        
        // 解析JSON
        let mappings: HashMap<String, String> = serde_json::from_str(&content)
            .map_err(|e| ConfigError::new(format!(
                "无法解析JSON配置文件 {}: {}",
                path_ref.display(),
                e
            )))?;
        
        Ok(TableMappingConfig { mappings })
    }
    
    /// 获取实体对应的表名，如果不存在则返回小写的实体名
    pub fn get_table_name(&self, entity: &str) -> String {
        self.mappings
            .get(entity)
            .cloned()
            .unwrap_or_else(|| entity.to_lowercase())
    }
    
    /// 获取所有映射
    pub fn get_mappings(&self) -> &HashMap<String, String> {
        &self.mappings
    }
    
    /// 创建默认配置（用于测试或fallback）
    pub fn default() -> Self {
        let mut mappings = HashMap::new();
        mappings.insert("Test".to_string(), "tests".to_string());
        mappings.insert("Run".to_string(), "test_runs".to_string());
        mappings.insert("Project".to_string(), "projects".to_string());
        mappings.insert("Task".to_string(), "tasks".to_string());
        mappings.insert("User".to_string(), "users".to_string());
        mappings.insert("Issue".to_string(), "issues".to_string());
        
        Self { mappings }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    
    #[test]
    fn test_load_valid_json_config() {
        // 创建临时配置文件
        let temp_file = "test_table_mapping.json";
        let mut file = fs::File::create(temp_file).unwrap();
        writeln!(file, r#"{{
            "Test": "tests",
            "Run": "test_runs",
            "Project": "projects"
        }}"#).unwrap();
        
        // 测试加载
        let config = TableMappingConfig::from_json_file(temp_file).unwrap();
        assert_eq!(config.get_table_name("Test"), "tests");
        assert_eq!(config.get_table_name("Run"), "test_runs");
        assert_eq!(config.get_table_name("Unknown"), "unknown");
        
        // 清理
        fs::remove_file(temp_file).ok();
    }
    
    #[test]
    fn test_invalid_json_config() {
        let temp_file = "test_invalid.json";
        let mut file = fs::File::create(temp_file).unwrap();
        writeln!(file, "invalid json").unwrap();
        
        let result = TableMappingConfig::from_json_file(temp_file);
        assert!(result.is_err());
        
        // 清理
        fs::remove_file(temp_file).ok();
    }
    
    #[test]
    fn test_missing_file() {
        let result = TableMappingConfig::from_json_file("non_existent_file.json");
        assert!(result.is_err());
    }
    
    #[test]
    fn test_default_config() {
        let config = TableMappingConfig::default();
        assert_eq!(config.get_table_name("Test"), "tests");
        assert_eq!(config.get_table_name("Unknown"), "unknown");
    }
} 