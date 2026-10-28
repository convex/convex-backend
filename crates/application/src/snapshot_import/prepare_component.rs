use anyhow::Context;
use async_recursion::async_recursion;
use common::{
    bootstrap_model::components::{
        definition::{
            ComponentDefinitionMetadata,
            ComponentDefinitionType,
        },
        ComponentMetadata,
        ComponentState,
        ComponentType,
    },
    components::{
        ComponentDefinitionPath,
        ComponentId,
        ComponentName,
        ComponentPath,
    },
    runtime::Runtime,
};
use database::{
    BootstrapComponentsModel,
    Database,
    SystemMetadataModel,
    Transaction,
    COMPONENTS_TABLE,
    SCHEMAS_TABLE,
};
use keybroker::Identity;
use maplit::{
    btreemap,
    btreeset,
};
use model::components::config::{
    ComponentConfigModel,
    ComponentDefinitionConfigModel,
};

#[async_recursion]
pub async fn prepare_component_for_import<RT>(
    database: &Database<RT>,
    component_path: &ComponentPath,
) -> anyhow::Result<ComponentId>
where
    RT: Runtime,
{
    let mut tx = database.begin(Identity::system()).await?;
    if let Some(metadata) = BootstrapComponentsModel::new(&mut tx).resolve_path(component_path)? {
        let component_id = if metadata.component_type.is_root() {
            ComponentId::Root
        } else {
            ComponentId::Child(metadata.developer_id())
        };
        return Ok(component_id);
    }

    let Some((parent_path, component_name)) = component_path.parent() else {
        tracing::info!("Creating a root component during import");
        create_root_component(&mut tx).await?;
        database
            .commit_with_write_source(tx, "snapshot_import_create_root_component")
            .await?;
        return Ok(ComponentId::Root);
    };
    drop(tx);

    prepare_component_for_import(database, &parent_path).await?;

    tracing::info!("Creating component {component_name:?} during import");
    let component_id = create_unmounted_component(database, parent_path, component_name).await?;
    Ok(component_id)
}

async fn create_unmounted_component<RT: Runtime>(
    database: &Database<RT>,
    parent_path: ComponentPath,
    component_name: ComponentName,
) -> anyhow::Result<ComponentId> {
    let mut tx = database.begin(Identity::system()).await?;
    let component_id = ComponentConfigModel::new(&mut tx)
        .initialize_component_namespace(false)
        .await?;
    database
        .commit_with_write_source(tx, "snapshot_import_prepare_unmounted_component")
        .await?;
    database
        .load_indexes_into_memory(btreeset! { SCHEMAS_TABLE.clone() })
        .await?;

    let mut tx = database.begin(Identity::system()).await?;
    let definition = ComponentDefinitionMetadata {
        path: format!("{}", parent_path.join(component_name.clone())).parse()?,
        definition_type: ComponentDefinitionType::ChildComponent {
            name: component_name.clone(),
            args: btreemap! {},
        },
        child_components: vec![],
        http_mounts: btreemap! {},
        http_prefix: None,
        exports: btreemap! {},
        env_vars: btreemap! {},
    };
    let (definition_id, _diff) = ComponentDefinitionConfigModel::new(&mut tx)
        .create_component_definition(definition)
        .await?;
    let metadata = ComponentMetadata {
        definition_id,
        component_type: ComponentType::ChildComponent {
            parent: BootstrapComponentsModel::new(&mut tx)
                .resolve_path(&parent_path)?
                .context(format!(
                    "{parent_path:?} not found in create_unmounted_component"
                ))?
                .developer_id(),
            name: component_name,
            args: btreemap! {},
            env: btreemap! {},
        },
        state: ComponentState::Unmounted,
        http_prefix: None,
    };
    SystemMetadataModel::new_global(&mut tx)
        .insert_with_internal_id(
            &COMPONENTS_TABLE,
            component_id.internal_id(),
            metadata.try_into()?,
        )
        .await?;
    database
        .commit_with_write_source(tx, "snapshot_import_insert_unmounted_component")
        .await?;
    Ok(ComponentId::Child(component_id))
}

async fn create_root_component<RT: Runtime>(tx: &mut Transaction<RT>) -> anyhow::Result<()> {
    let component_id = ComponentConfigModel::new(tx)
        .initialize_component_namespace(true)
        .await?;

    let definition = ComponentDefinitionMetadata {
        path: ComponentDefinitionPath::root(),
        definition_type: ComponentDefinitionType::App,
        child_components: vec![],
        http_mounts: btreemap! {},
        http_prefix: None,
        exports: btreemap! {},
        env_vars: btreemap! {},
    };

    let (definition_id, _diff) = ComponentDefinitionConfigModel::new(tx)
        .create_component_definition(definition)
        .await?;
    let metadata = ComponentMetadata {
        definition_id,
        component_type: ComponentType::App,
        state: ComponentState::Active,
        http_prefix: None,
    };
    SystemMetadataModel::new_global(tx)
        .insert_with_internal_id(
            &COMPONENTS_TABLE,
            component_id.internal_id(),
            metadata.try_into()?,
        )
        .await?;
    Ok(())
}
