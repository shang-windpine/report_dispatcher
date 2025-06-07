/// AST 的根节点, 代表一个完整的查询
#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    /// 针对主实体的过滤条件
    pub base_filters: Vec<FieldFilter>,
    /// 针对关联实体的过滤条件
    pub cross_filters: Vec<CrossFilter>,
}

/// 代表一个关联实体过滤, e.g., `CrossFilter: <Test-Run>...`
#[derive(Debug, Clone, PartialEq)]
pub struct CrossFilter {
    pub source_entity: Identifier,
    pub target_entity: Identifier,
    /// 应用于目标实体的过滤条件列表
    pub filters: Vec<FieldFilter>,
}

/// 代表对单个字段的一个或多个过滤条件, e.g., `status[NOT "Open"]`
#[derive(Debug, Clone, PartialEq)]
pub struct FieldFilter {
    pub field: Identifier,
    pub condition: Condition,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Identifier(pub String);

/// 代表应用于单个字段的条件树
#[derive(Debug, Clone, PartialEq)]
pub enum Condition {
    /// 逻辑与 (AND)
    And(Box<Condition>, Box<Condition>),
    /// 逻辑或 (OR)
    Or(Box<Condition>, Box<Condition>),
    /// 逻辑非 (NOT)
    Not(Box<Condition>),
    /// 使用括号分组的条件
    Grouped(Box<Condition>),
    /// 基础比较, 这是条件的叶子节点
    Comparison { op: CompOp, value: Literal },
    /// IN (...) 检查
    In(Vec<Literal>),
    /// 空值检查
    IsNull,
    IsNotNull,
}

/// 比较操作符
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompOp {
    Eq,      // =
    NotEq,   // !=
    Gt,      // >
    Lt,      // <
    Gte,     // >=
    Lte,     // <=
}

/// 字面量
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    String(String),
    Number(i64),
    Date(String), // e.g., "2023-12-25" or a resolved keyword like "today"
    CurrentUser,
} 