use std::collections::HashMap;

use super::{
    super::tests::{
        change,
        memory_catalog,
        read,
    },
    *,
};

fn source(table: &str) -> SourceTable {
    component_source("", table)
}

fn component_source(component: &str, table: &str) -> SourceTable {
    SourceTable {
        component: component.to_owned(),
        table: table.to_owned(),
    }
}

async fn open(catalog: Arc<dyn Catalog>) -> Result<IcebergChangeWriter> {
    open_selecting(catalog, vec![]).await
}

async fn open_selecting(
    catalog: Arc<dyn Catalog>,
    selected: Vec<SourceTable>,
) -> Result<IcebergChangeWriter> {
    IcebergChangeWriter::open(
        catalog,
        "convex_chess_wandering_fish_513",
        "memory://warehouse",
        selected,
    )
    .await
}

async fn changes(writer: &IcebergChangeWriter, table: &str) -> Result<Vec<Change>> {
    let ident = TableIdent::new(
        writer.namespace.clone(),
        IcebergChangeWriter::table_name(&source(table))?,
    );
    read(&writer.catalog.load_table(&ident).await?).await
}

#[tokio::test]
async fn truncates_recreate_tables_and_changes_append() -> Result<()> {
    let catalog = memory_catalog().await?;
    let mut writer = open(catalog.clone()).await?;
    writer
        .apply(
            &[source("menu"), source("empty")],
            vec![(source("menu"), change("burger", 10, Some("10")))],
        )
        .await?;
    writer
        .apply(&[], vec![(source("menu"), change("burger", 12, None))])
        .await?;
    assert_eq!(
        changes(&writer, "menu").await?,
        vec![change("burger", 10, Some("10")), change("burger", 12, None)]
    );
    assert!(changes(&writer, "empty").await?.is_empty());
    let properties = writer.tables[&source("menu")].metadata().properties();
    assert_eq!(properties[COMPONENT_PROPERTY], "");
    assert_eq!(properties[TABLE_PROPERTY], "menu");

    writer
        .apply(
            &[source("menu")],
            vec![(source("menu"), change("soup", 5, Some("6")))],
        )
        .await?;
    assert_eq!(
        changes(&writer, "menu").await?,
        vec![change("soup", 5, Some("6"))]
    );

    let mut reopened = open(catalog.clone()).await?;
    reopened
        .apply(
            &[],
            vec![(source("menu"), change("burger", 20, Some("12")))],
        )
        .await?;
    assert_eq!(
        changes(&reopened, "menu").await?,
        vec![
            change("burger", 20, Some("12")),
            change("soup", 5, Some("6"))
        ]
    );
    Ok(())
}

#[tokio::test]
async fn locations_track_every_commit_and_summaries_name_the_page() -> Result<()> {
    let catalog = memory_catalog().await?;
    let mut writer = open(catalog.clone()).await?;
    writer.snapshot_properties = HashMap::from([("convex.page".to_owned(), "7".to_owned())]);
    writer
        .apply(
            &[source("menu"), source("empty")],
            vec![(source("menu"), change("burger", 10, Some("10")))],
        )
        .await?;
    for name in ["app__menu", "app__empty"] {
        let table = catalog
            .load_table(&TableIdent::new(writer.namespace.clone(), name.to_owned()))
            .await?;
        assert_eq!(
            writer.locations()[name],
            table.metadata_location_result()?,
            "{name}"
        );
    }
    let menu = &writer.tables[&source("menu")];
    let snapshot = menu.metadata().current_snapshot().unwrap();
    assert_eq!(snapshot.summary().additional_properties["convex.page"], "7");
    let after_one_row = writer.bytes_written();
    assert!(after_one_row > 0, "one Parquet file was written");

    writer.apply(&[source("menu")], vec![]).await?;
    assert_eq!(
        writer.bytes_written(),
        after_one_row,
        "recreating a table writes metadata only"
    );
    let menu = catalog
        .load_table(&TableIdent::new(
            writer.namespace.clone(),
            "app__menu".to_owned(),
        ))
        .await?;
    assert_eq!(
        writer.locations()["app__menu"],
        menu.metadata_location_result()?
    );
    Ok(())
}

#[tokio::test]
async fn names_follow_components_and_keep_case_in_directories() -> Result<()> {
    let mut writer = open(memory_catalog().await?).await?;
    let node = component_source("aggregate/inner", "btreeNode");
    writer
        .apply(&[source("authRefreshTokens"), node.clone()], vec![])
        .await?;
    assert_eq!(writer.tables.len(), 2);
    for (source, name, location) in [
        (
            &node,
            "aggregate__inner__btreenode",
            "memory://warehouse/aggregate__inner/btreeNode",
        ),
        (
            &source("authRefreshTokens"),
            "app__authrefreshtokens",
            "memory://warehouse/app/authRefreshTokens",
        ),
    ] {
        let table = &writer.tables[source];
        assert_eq!(table.identifier().name(), name);
        assert_eq!(table.metadata().location(), location);
    }
    Ok(())
}

#[tokio::test]
async fn colliding_sources_fail_before_any_write() -> Result<()> {
    let mut writer = open(memory_catalog().await?).await?;
    let error = writer
        .apply(&[source("Users"), source("users")], vec![])
        .await
        .expect_err("a colliding page must be rejected");
    assert!(error.to_string().contains("both map to"), "{error:#}");
    assert!(writer.tables.is_empty());
    writer.apply(&[source("menu")], vec![]).await?;
    let error = open_selecting(
        writer.catalog.clone(),
        vec![source("Users"), source("users")],
    )
    .await
    .err()
    .expect("a colliding selection must be rejected");
    assert!(error.to_string().contains("both map to"), "{error:#}");
    assert_eq!(
        writer.catalog.list_tables(&writer.namespace).await?.len(),
        1
    );
    let deep = "s3://b/".to_owned() + &"p/".repeat(450);
    let error = IcebergChangeWriter::check_names(&deep, &[source("users")])
        .expect_err("an overlong location must be rejected");
    assert!(error.to_string().contains("longer than"), "{error:#}");
    Ok(())
}

#[tokio::test]
async fn a_new_table_replaces_a_leftover_with_the_same_name() -> Result<()> {
    let mut writer = open(memory_catalog().await?).await?;
    writer
        .apply(
            &[source("Users")],
            vec![(source("Users"), change("ada", 1, Some("{}")))],
        )
        .await?;
    for replacement in [source("users"), component_source("app", "users")] {
        writer = open(writer.catalog.clone()).await?;
        let error = writer
            .apply(
                &[],
                vec![(replacement.clone(), change("grace", 2, Some("{}")))],
            )
            .await
            .expect_err("a different source must truncate before reusing a Glue name");
        assert!(
            error.to_string().contains("belongs to another source"),
            "{error:#}"
        );
        writer
            .apply(
                std::slice::from_ref(&replacement),
                vec![(replacement.clone(), change("grace", 2, Some("{}")))],
            )
            .await?;
        assert_eq!(writer.tables.keys().collect::<Vec<_>>(), [&replacement]);
        assert_eq!(
            read(&writer.tables[&replacement]).await?,
            vec![change("grace", 2, Some("{}"))]
        );
    }
    Ok(())
}

#[tokio::test]
async fn retirement_follows_the_selection() -> Result<()> {
    let catalog = memory_catalog().await?;
    let mut writer = open(catalog).await?;
    writer
        .apply(&[source("menu"), source("orders")], vec![])
        .await?;
    let retired = |writer: &IcebergChangeWriter, table: &str| {
        writer.tables[&source(table)]
            .metadata()
            .properties()
            .get(RETIRED_PROPERTY)
            .cloned()
    };
    writer = open(writer.catalog.clone()).await?;
    writer.retire_unselected(&[source("menu")]).await?;
    assert_eq!(retired(&writer, "menu"), None);
    assert_eq!(retired(&writer, "orders").as_deref(), Some("true"));
    writer
        .retire_unselected(&[source("menu"), source("orders")])
        .await?;
    assert_eq!(retired(&writer, "orders").as_deref(), Some("false"));
    Ok(())
}

#[tokio::test]
async fn foreign_tables_are_checked_when_used_or_reconciled() -> Result<()> {
    let catalog = memory_catalog().await?;
    open(catalog.clone()).await?;
    catalog
        .create_table(
            &NamespaceIdent::new("convex_chess_wandering_fish_513".to_owned()),
            TableCreation::builder()
                .name("app__menu".to_owned())
                .schema(change_log_schema()?)
                .build(),
        )
        .await?;
    let mut writer = open(catalog).await?;
    writer
        .apply(
            &[],
            vec![(source("orders"), change("order", 1, Some("{}")))],
        )
        .await?;
    assert_eq!(changes(&writer, "orders").await?.len(), 1);

    let error = writer
        .apply(&[], vec![(source("menu"), change("burger", 1, Some("{}")))])
        .await
        .expect_err("appending to a foreign table must fail");
    assert!(error.to_string().contains("not written by"), "{error:#}");
    let error = writer
        .apply(&[source("menu")], vec![])
        .await
        .expect_err("truncating a foreign table must fail");
    assert!(error.to_string().contains("not written by"), "{error:#}");
    let error = writer
        .retire_unselected(&[])
        .await
        .expect_err("retirement must validate the full catalog");
    assert!(error.to_string().contains("not written by"), "{error:#}");
    Ok(())
}

#[tokio::test]
async fn a_known_retired_set_loads_only_the_tables_a_page_uses() -> Result<()> {
    let catalog = memory_catalog().await?;
    let mut writer = open(catalog.clone()).await?;
    writer
        .apply(&[source("menu"), source("orders")], vec![])
        .await?;
    writer
        .retire_unselected(&[source("menu"), source("orders")])
        .await?;
    let locations = writer.locations().clone();
    assert_eq!(writer.retired(), Some(&BTreeSet::new()));

    // Forget the tables, as a fresh staged writer starts.
    for name in locations.keys() {
        catalog
            .drop_table(&TableIdent::new(writer.namespace.clone(), name.clone()))
            .await?;
    }
    let mut writer = open(catalog.clone()).await?;
    writer.locations = locations.clone();
    writer.unregistered = locations;
    writer.retired = Some(BTreeSet::new());
    let registered = async |writer: &IcebergChangeWriter| -> Result<Vec<String>> {
        Ok(catalog
            .list_tables(&writer.namespace)
            .await?
            .into_iter()
            .map(|ident| ident.name().to_owned())
            .collect())
    };

    writer
        .apply(&[], vec![(source("menu"), change("burger", 1, Some("{}")))])
        .await?;
    writer
        .retire_unselected(&[source("menu"), source("orders")])
        .await?;
    assert_eq!(registered(&writer).await?, ["app__menu"]);

    writer.retire_unselected(&[source("menu")]).await?;
    assert_eq!(registered(&writer).await?.len(), 2);
    assert_eq!(
        writer.retired(),
        Some(&BTreeSet::from(["app__orders".to_owned()]))
    );
    assert_eq!(
        writer.tables[&source("orders")].metadata().properties()[RETIRED_PROPERTY],
        "true"
    );
    Ok(())
}
