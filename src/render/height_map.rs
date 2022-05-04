use crate::{
    render::{
        gpu_node_atlas::{GpuNodeAtlas, NodeAttachment},
        InitTerrain,
    },
    PersistentComponent, TerrainConfig,
};
use bevy::{
    prelude::*,
    render::{
        render_asset::RenderAssets,
        render_resource::*,
        renderer::{RenderDevice, RenderQueue},
    },
};

/// Runs in prepare.
pub(crate) fn init_height_attachment(
    device: Res<RenderDevice>,
    mut gpu_node_atlases: ResMut<PersistentComponent<GpuNodeAtlas>>,
    terrain_query: Query<(Entity, &TerrainConfig), With<InitTerrain>>,
) {
    for (entity, config) in terrain_query.iter() {
        info!("initializing height attachment");

        let texture = device.create_texture(&TextureDescriptor {
            label: None,
            size: Extent3d {
                width: config.texture_size,
                height: config.texture_size,
                depth_or_array_layers: config.node_atlas_size as u32,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::R16Unorm,
            usage: TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING,
        });

        let sampler = device.create_sampler(&SamplerDescriptor {
            label: None,
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_filter: FilterMode::Linear,
            lod_min_clamp: 0.0,
            lod_max_clamp: f32::MAX,
            compare: None,
            anisotropy_clamp: None,
            border_color: None,
        });

        let view = texture.create_view(&TextureViewDescriptor {
            label: None,
            format: None,
            dimension: Some(TextureViewDimension::D2Array),
            aspect: TextureAspect::All,
            base_mip_level: 0,
            mip_level_count: None,
            base_array_layer: 0,
            array_layer_count: None,
        });

        let gpu_node_atlas = gpu_node_atlases.get_mut(&entity).unwrap();
        gpu_node_atlas.atlas_attachments.insert(
            "heightmap".into(),
            NodeAttachment::Texture {
                texture,
                view,
                sampler,
            },
        );
    }
}

pub(crate) fn queue_height_attachment_updates(
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    images: Res<RenderAssets<Image>>,
    mut gpu_node_atlases: ResMut<PersistentComponent<GpuNodeAtlas>>,
    terrain_query: Query<(Entity, &TerrainConfig), ()>,
) {
    let mut command_encoder = device.create_command_encoder(&CommandEncoderDescriptor::default());

    for (entity, config) in terrain_query.iter() {
        let gpu_node_atlas = gpu_node_atlases.get_mut(&entity).unwrap();

        for (index, node_data) in &gpu_node_atlas.activated_nodes {
            let image = images.get(&node_data.height_map).unwrap();

            let height_texture = gpu_node_atlas
                .atlas_attachments
                .get("heightmap".into())
                .unwrap();

            let height_texture = match height_texture {
                NodeAttachment::Buffer(_) => continue,
                NodeAttachment::Texture { texture, .. } => texture,
            };

            command_encoder.copy_texture_to_texture(
                ImageCopyTexture {
                    texture: &image.texture,
                    mip_level: 0,
                    origin: Origin3d { x: 0, y: 0, z: 0 },
                    aspect: TextureAspect::All,
                },
                ImageCopyTexture {
                    texture: height_texture,
                    mip_level: 0,
                    origin: Origin3d {
                        x: 0,
                        y: 0,
                        z: *index as u32,
                    },
                    aspect: TextureAspect::All,
                },
                Extent3d {
                    width: config.texture_size,
                    height: config.texture_size,
                    depth_or_array_layers: 1,
                },
            );
        }
    }

    queue.submit(vec![command_encoder.finish()]);
}