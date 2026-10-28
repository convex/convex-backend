use std::str::FromStr;

use common::types::PersistenceVersion;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DbDriverTag {
    Sqlite,
    Postgres(PersistenceVersion),
    MySql(PersistenceVersion),
    MySqlMultitenant(PersistenceVersion),
}

impl clap::ValueEnum for DbDriverTag {
    fn value_variants<'a>() -> &'a [Self] {
        &[
            DbDriverTag::Sqlite,
            DbDriverTag::MySql(PersistenceVersion::V5),
            DbDriverTag::MySqlMultitenant(PersistenceVersion::V5),
            DbDriverTag::MySqlMultitenant(PersistenceVersion::V6),
            DbDriverTag::Postgres(PersistenceVersion::V5),
        ]
    }

    fn to_possible_value(&self) -> Option<clap::builder::PossibleValue> {
        Some(clap::builder::PossibleValue::new(self.as_str()))
    }
}

impl DbDriverTag {
    pub fn persistence_version(&self) -> anyhow::Result<PersistenceVersion> {
        match self {
            Self::Postgres(version) | Self::MySql(version) | Self::MySqlMultitenant(version) => {
                Ok(*version)
            },
            Self::Sqlite => {
                anyhow::bail!("sqlite has no persistence version")
            },
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            DbDriverTag::Sqlite => "sqlite",
            DbDriverTag::Postgres(PersistenceVersion::V5) => "postgres-v5",
            DbDriverTag::MySql(PersistenceVersion::V5) => "mysql-v5",
            DbDriverTag::MySqlMultitenant(PersistenceVersion::V5) => "mysql-v5-multitenant",
            DbDriverTag::MySqlMultitenant(PersistenceVersion::V6) => "mysql-v6-multitenant",
            DbDriverTag::Postgres(PersistenceVersion::V6)
            | DbDriverTag::MySql(PersistenceVersion::V6) => {
                unreachable!("V6 is only supported on MySQL multitenant")
            },
        }
    }
}

impl FromStr for DbDriverTag {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "sqlite" => Ok(Self::Sqlite),
            "postgres-v5" => Ok(DbDriverTag::Postgres(PersistenceVersion::V5)),
            "mysql-v5" => Ok(DbDriverTag::MySql(PersistenceVersion::V5)),
            "mysql-v5-multitenant" => Ok(DbDriverTag::MySqlMultitenant(PersistenceVersion::V5)),
            "mysql-v6-multitenant" => Ok(DbDriverTag::MySqlMultitenant(PersistenceVersion::V6)),
            _ => anyhow::bail!("unrecognized db_driver {s}"),
        }
    }
}
