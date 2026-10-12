use std::collections::HashSet;

pub const MAX_FORMULA_EXPRESSION_CHARS: usize = 2_000;
const MAX_AST_NODES: usize = 256;
const MAX_FUNCTION_ARGS: usize = 16;

#[derive(Clone, Debug)]
pub enum FormulaExpr {
    Number(f64),
    String(String),
    Boolean(bool),
    Unary {
        op: UnaryOp,
        expr: Box<FormulaExpr>,
    },
    Binary {
        op: BinaryOp,
        left: Box<FormulaExpr>,
        right: Box<FormulaExpr>,
    },
    Call {
        name: String,
        args: Vec<FormulaExpr>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnaryOp {
    Negate,
    Not,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    And,
    Or,
}

pub fn parse_formula_expression(expression: &str) -> Result<FormulaExpr, String> {
    if expression.trim().is_empty() {
        return Err("formula.expression is required".to_string());
    }
    if expression.chars().count() > MAX_FORMULA_EXPRESSION_CHARS {
        return Err("formula.expression is too long".to_string());
    }
    let mut parser = FormulaParser::new(expression);
    let expr = parser.parse_expression()?;
    parser.skip_whitespace();
    if !parser.is_at_end() {
        return Err("formula.expression contains unexpected trailing input".to_string());
    }
    Ok(expr)
}

pub fn formula_dependencies(expr: &FormulaExpr) -> Result<HashSet<String>, String> {
    let mut dependencies = HashSet::new();
    collect_dependencies(expr, &mut dependencies)?;
    if dependencies.len() > 32 {
        return Err("formula references too many properties".to_string());
    }
    Ok(dependencies)
}

fn collect_dependencies(
    expr: &FormulaExpr,
    dependencies: &mut HashSet<String>,
) -> Result<(), String> {
    match expr {
        FormulaExpr::Call { name, args } if name.eq_ignore_ascii_case("prop") => {
            let [FormulaExpr::String(property_name)] = args.as_slice() else {
                return Err("prop() requires one literal property name".to_string());
            };
            if property_name.trim().is_empty() {
                return Err("prop() property name is required".to_string());
            }
            dependencies.insert(property_name.clone());
        }
        FormulaExpr::Call { args, .. } => {
            for arg in args {
                collect_dependencies(arg, dependencies)?;
            }
        }
        FormulaExpr::Unary { expr, .. } => collect_dependencies(expr, dependencies)?,
        FormulaExpr::Binary { left, right, .. } => {
            collect_dependencies(left, dependencies)?;
            collect_dependencies(right, dependencies)?;
        }
        FormulaExpr::Number(_) | FormulaExpr::String(_) | FormulaExpr::Boolean(_) => {}
    }
    Ok(())
}

struct FormulaParser {
    chars: Vec<char>,
    pos: usize,
    node_count: usize,
}

impl FormulaParser {
    fn new(expression: &str) -> Self {
        Self {
            chars: expression.chars().collect(),
            pos: 0,
            node_count: 0,
        }
    }

    fn parse_expression(&mut self) -> Result<FormulaExpr, String> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<FormulaExpr, String> {
        let mut expr = self.parse_and()?;
        loop {
            if self.match_token("||") || self.match_keyword("or") {
                let right = self.parse_and()?;
                expr = self.binary(BinaryOp::Or, expr, right)?;
            } else {
                return Ok(expr);
            }
        }
    }

    fn parse_and(&mut self) -> Result<FormulaExpr, String> {
        let mut expr = self.parse_equality()?;
        loop {
            if self.match_token("&&") || self.match_keyword("and") {
                let right = self.parse_equality()?;
                expr = self.binary(BinaryOp::And, expr, right)?;
            } else {
                return Ok(expr);
            }
        }
    }

    fn parse_equality(&mut self) -> Result<FormulaExpr, String> {
        let mut expr = self.parse_comparison()?;
        loop {
            if self.match_token("==") {
                let right = self.parse_comparison()?;
                expr = self.binary(BinaryOp::Equal, expr, right)?;
            } else if self.match_token("!=") {
                let right = self.parse_comparison()?;
                expr = self.binary(BinaryOp::NotEqual, expr, right)?;
            } else {
                return Ok(expr);
            }
        }
    }

    fn parse_comparison(&mut self) -> Result<FormulaExpr, String> {
        let mut expr = self.parse_term()?;
        loop {
            if self.match_token("<=") {
                let right = self.parse_term()?;
                expr = self.binary(BinaryOp::LessEqual, expr, right)?;
            } else if self.match_token(">=") {
                let right = self.parse_term()?;
                expr = self.binary(BinaryOp::GreaterEqual, expr, right)?;
            } else if self.match_token("<") {
                let right = self.parse_term()?;
                expr = self.binary(BinaryOp::Less, expr, right)?;
            } else if self.match_token(">") {
                let right = self.parse_term()?;
                expr = self.binary(BinaryOp::Greater, expr, right)?;
            } else {
                return Ok(expr);
            }
        }
    }

    fn parse_term(&mut self) -> Result<FormulaExpr, String> {
        let mut expr = self.parse_factor()?;
        loop {
            if self.match_token("+") {
                let right = self.parse_factor()?;
                expr = self.binary(BinaryOp::Add, expr, right)?;
            } else if self.match_token("-") {
                let right = self.parse_factor()?;
                expr = self.binary(BinaryOp::Subtract, expr, right)?;
            } else {
                return Ok(expr);
            }
        }
    }

    fn parse_factor(&mut self) -> Result<FormulaExpr, String> {
        let mut expr = self.parse_unary()?;
        loop {
            if self.match_token("*") {
                let right = self.parse_unary()?;
                expr = self.binary(BinaryOp::Multiply, expr, right)?;
            } else if self.match_token("/") {
                let right = self.parse_unary()?;
                expr = self.binary(BinaryOp::Divide, expr, right)?;
            } else if self.match_token("%") {
                let right = self.parse_unary()?;
                expr = self.binary(BinaryOp::Modulo, expr, right)?;
            } else {
                return Ok(expr);
            }
        }
    }

    fn parse_unary(&mut self) -> Result<FormulaExpr, String> {
        if self.match_token("-") {
            let expr = self.parse_unary()?;
            return self.unary(UnaryOp::Negate, expr);
        }
        if self.match_token("!") || self.match_keyword("not") {
            let expr = self.parse_unary()?;
            return self.unary(UnaryOp::Not, expr);
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<FormulaExpr, String> {
        self.skip_whitespace();
        if self.match_token("(") {
            let expr = self.parse_expression()?;
            if !self.match_token(")") {
                return Err("formula.expression has an unclosed parenthesis".to_string());
            }
            return Ok(expr);
        }
        if self.peek() == Some('"') {
            return self.string();
        }
        if self.peek().is_some_and(|value| value.is_ascii_digit()) {
            return self.number();
        }
        if self.peek().is_some_and(is_identifier_start) {
            return self.identifier_or_call();
        }
        Err("formula.expression expected a value".to_string())
    }

    fn identifier_or_call(&mut self) -> Result<FormulaExpr, String> {
        let name = self.identifier();
        if name == "true" {
            return self.expr(FormulaExpr::Boolean(true));
        }
        if name == "false" {
            return self.expr(FormulaExpr::Boolean(false));
        }
        if !self.match_token("(") {
            return Err("formula identifiers must be function calls".to_string());
        }
        let mut args = Vec::new();
        if !self.check(")") {
            loop {
                if args.len() >= MAX_FUNCTION_ARGS {
                    return Err("formula function has too many arguments".to_string());
                }
                args.push(self.parse_expression()?);
                if !self.match_token(",") {
                    break;
                }
            }
        }
        if !self.match_token(")") {
            return Err("formula function call has an unclosed parenthesis".to_string());
        }
        self.expr(FormulaExpr::Call { name, args })
    }

    fn number(&mut self) -> Result<FormulaExpr, String> {
        let start = self.pos;
        while self.peek().is_some_and(|value| value.is_ascii_digit()) {
            self.pos += 1;
        }
        if self.peek() == Some('.') && self.peek_next().is_some_and(|value| value.is_ascii_digit())
        {
            self.pos += 1;
            while self.peek().is_some_and(|value| value.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        let text = self.chars[start..self.pos].iter().collect::<String>();
        let number = text
            .parse::<f64>()
            .map_err(|_| "formula number literal is invalid".to_string())?;
        if !number.is_finite() {
            return Err("formula number literal must be finite".to_string());
        }
        self.expr(FormulaExpr::Number(number))
    }

    fn string(&mut self) -> Result<FormulaExpr, String> {
        self.pos += 1;
        let mut value = String::new();
        while let Some(ch) = self.peek() {
            self.pos += 1;
            match ch {
                '"' => return self.expr(FormulaExpr::String(value)),
                '\\' => {
                    let Some(escaped) = self.peek() else {
                        return Err("formula string escape is incomplete".to_string());
                    };
                    self.pos += 1;
                    match escaped {
                        '"' => value.push('"'),
                        '\\' => value.push('\\'),
                        'n' => value.push('\n'),
                        'r' => value.push('\r'),
                        't' => value.push('\t'),
                        other => value.push(other),
                    }
                }
                other if other.is_control() => {
                    return Err("formula string must not contain control characters".to_string());
                }
                other => value.push(other),
            }
            if value.chars().count() > MAX_FORMULA_EXPRESSION_CHARS {
                return Err("formula string literal is too long".to_string());
            }
        }
        Err("formula string literal is unclosed".to_string())
    }

    fn identifier(&mut self) -> String {
        let start = self.pos;
        self.pos += 1;
        while self.peek().is_some_and(is_identifier_part) {
            self.pos += 1;
        }
        self.chars[start..self.pos].iter().collect()
    }

    fn unary(&mut self, op: UnaryOp, expr: FormulaExpr) -> Result<FormulaExpr, String> {
        self.expr(FormulaExpr::Unary {
            op,
            expr: Box::new(expr),
        })
    }

    fn binary(
        &mut self,
        op: BinaryOp,
        left: FormulaExpr,
        right: FormulaExpr,
    ) -> Result<FormulaExpr, String> {
        self.expr(FormulaExpr::Binary {
            op,
            left: Box::new(left),
            right: Box::new(right),
        })
    }

    fn expr(&mut self, expr: FormulaExpr) -> Result<FormulaExpr, String> {
        self.node_count += 1;
        if self.node_count > MAX_AST_NODES {
            return Err("formula expression is too complex".to_string());
        }
        Ok(expr)
    }

    fn match_keyword(&mut self, keyword: &str) -> bool {
        self.skip_whitespace();
        let end = self.pos + keyword.chars().count();
        if end > self.chars.len() {
            return false;
        }
        let text = self.chars[self.pos..end].iter().collect::<String>();
        if text != keyword {
            return false;
        }
        if self
            .chars
            .get(end)
            .is_some_and(|value| is_identifier_part(*value))
        {
            return false;
        }
        self.pos = end;
        true
    }

    fn match_token(&mut self, token: &str) -> bool {
        self.skip_whitespace();
        if !self.check(token) {
            return false;
        }
        self.pos += token.chars().count();
        true
    }

    fn check(&mut self, token: &str) -> bool {
        self.skip_whitespace();
        let end = self.pos + token.chars().count();
        end <= self.chars.len() && self.chars[self.pos..end].iter().collect::<String>() == token
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.pos += 1;
        }
    }

    fn is_at_end(&self) -> bool {
        self.pos >= self.chars.len()
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_next(&self) -> Option<char> {
        self.chars.get(self.pos + 1).copied()
    }
}

fn is_identifier_start(value: char) -> bool {
    value.is_ascii_alphabetic() || value == '_'
}

fn is_identifier_part(value: char) -> bool {
    value.is_ascii_alphanumeric() || value == '_'
}
