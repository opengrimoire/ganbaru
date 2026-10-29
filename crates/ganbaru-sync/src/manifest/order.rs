//! Dependency order of replicated tables.
//!
//! A dependency runs from a table to a table whose rows it needs: the target of a reference,
//! the target of a foreign key of one of its owned child tables, or a table one of its
//! `ApplierVisible` triggers reads. The manifest lists the strongly connected components of
//! these dependencies in topological order, each component contiguous.
//!
//! The engine applies with foreign keys deferred, so the tables of one component materialize
//! together and reference cycles of any `on_delete` are allowed. Owned rows and trigger checks
//! see rows as each statement leaves them, so owned and `ApplierVisible` dependencies never lie
//! on a cycle, including a trigger that reads its own table.

use std::collections::BTreeSet;

use ganbaru_sync_contracts::TableId;

use super::Manifest;

/// Why one replicated table needs the rows of another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DependencyKind {
    /// A reference column targets the other table.
    Reference,
    /// An owned child table has a foreign key into the other table.
    Owned,
    /// An `ApplierVisible` trigger reads the other table.
    ApplierVisible,
}

/// One table needing the rows of another, possibly itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Dependency {
    /// The dependent table.
    pub from: TableId,
    /// The table it needs.
    pub to: TableId,
    /// What creates the dependency.
    pub kind: DependencyKind,
}

impl Manifest {
    /// The dependencies the manifest declares itself: one per reference column, key parent, and
    /// owned reference.
    pub fn reference_dependencies(&self) -> Vec<Dependency> {
        self.tables
            .iter()
            .flat_map(|table| {
                let parents = table
                    .references()
                    .map(|(_, _, reference)| reference.table)
                    .chain(table.key_parent);
                let columns = parents.map(|to| Dependency {
                    from: table.id,
                    to,
                    kind: DependencyKind::Reference,
                });
                let owned = table
                    .owned_references()
                    .map(|(_, _, _, reference)| Dependency {
                        from: table.id,
                        to: reference.table,
                        kind: DependencyKind::Owned,
                    });
                columns.chain(owned)
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// The strongly connected component of each table under `dependencies`, by table
    /// position, named by the position of its first table. Dependencies on tables the
    /// manifest does not hold are ignored.
    pub fn components(&self, dependencies: &[Dependency]) -> Vec<usize> {
        let count = self.tables.len();
        let mut edges = vec![Vec::new(); count];
        for dependency in dependencies {
            if let (Some(from), Some(to)) =
                (self.position(dependency.from), self.position(dependency.to))
            {
                edges[from].push(to);
            }
        }
        let reach = (0..count)
            .map(|start| {
                let mut seen = vec![false; count];
                let mut stack = vec![start];
                seen[start] = true;
                while let Some(node) = stack.pop() {
                    for &next in &edges[node] {
                        if !seen[next] {
                            seen[next] = true;
                            stack.push(next);
                        }
                    }
                }
                seen
            })
            .collect::<Vec<_>>();
        (0..count)
            .map(|table| {
                (0..count)
                    .find(|&other| reach[table][other] && reach[other][table])
                    .unwrap_or(table)
            })
            .collect()
    }

    /// Why the table order does not fit `dependencies`: a dependency on an unknown table, a
    /// component that is not contiguous, a component listed after one that depends on it, or
    /// an owned or `ApplierVisible` dependency on a cycle.
    pub fn order_problems(&self, dependencies: &[Dependency]) -> Vec<String> {
        let mut problems = Vec::new();
        let components = self.components(dependencies);
        for (position, table) in self.tables.iter().enumerate() {
            let component = components[position];
            if position > 0 && components[position - 1] != component && component < position {
                problems.push(format!(
                    "{}: its dependency component is not contiguous",
                    table.name
                ));
            }
        }
        for dependency in dependencies {
            let (Some(from), Some(to)) =
                (self.position(dependency.from), self.position(dependency.to))
            else {
                problems.push(format!(
                    "table {} depends on table {}, which the manifest does not hold",
                    dependency.from.0, dependency.to.0
                ));
                continue;
            };
            let (from_name, to_name) = (self.tables[from].name, self.tables[to].name);
            let cyclic = from == to || components[from] == components[to];
            if cyclic && dependency.kind != DependencyKind::Reference {
                problems.push(format!(
                    "{from_name}: its {:?} dependency on {to_name} lies on a cycle",
                    dependency.kind
                ));
            } else if !cyclic && to > from {
                problems.push(format!(
                    "{from_name}: depends on {to_name}, which is listed later outside its component"
                ));
            }
        }
        problems
    }

    fn position(&self, id: TableId) -> Option<usize> {
        self.tables.iter().position(|table| table.id == id)
    }
}
