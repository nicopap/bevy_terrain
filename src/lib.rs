//! This crate provides the ability to render beautiful height-field terrains of any size.
//! This is achieved in extensible and modular manner, so that the terrain data
//! can be accessed from nearly anywhere (systems, shaders) [^note].
//!
//! # Background
//! There are three critical questions that each terrain renderer has to solve:
//!
//! ## How to store, manage and access the terrain data?
//! Each terrain has different types of textures associated with it.
//! For example a simple one might only need height and albedo information.
//! Because terrains can be quite large the space required for all of these so called
//! attachments, can/should not be stored in RAM and VRAM all at once.
//! Thus they have to be streamed in and out depending on the positions of the
//! viewers (cameras, lights, etc.).
//! Therefore the terrain is subdivided into a giant quadtree, whose nodes store their
//! section of these attachments.
//! This crate uses the chunked clipmap data structure, which consist of two pieces working together.
//! The wrapping [`Quadtree`](terrain_data::quadtree::Quadtree) views together with
//! the [`NodeAtlas`](terrain_data::node_atlas::NodeAtlas) (the data structure
//! that stores all of the currently loaded data) can be used to efficiently retrieve
//! the best currently available data at any position for terrains of any size.
//! See the [`terrain_data`] module for more information.
//!
//! ## How to best approximate the terrain geometry?
//! Even a small terrain with a height map of 1000x1000 pixels would require 1 million vertices
//! to be rendered each frame per view, with an naive approach without an lod strategy.
//! To better distribute the vertices over the screen there exist many different algorithms.
//! This crate comes with its own default terrain geometry algorithm, called the
//! Uniform Distance-Dependent Level of Detail (UDLOD), which was developed with performance and
//! quality scalability in mind.
//! See the [`render`] module for more information.
//! You can also implement a different algorithm yourself and only use the terrain
//! data structures to solve the first question.
//!
//! ## How to shade the terrain?
//! The third and most important challenge of terrain rendering is the shading. This is a very
//! project specific problem and thus there does not exist a one-size-fits-all solution.
//! You can define your own terrain [Material](bevy::pbr::Material) and shader with all the
//! detail textures tailored to your application.
//! In the future this plugin will provide modular shader functions to make techniques like splat
//! mapping, triplane mapping, etc. easier.
//! Additionally a virtual texturing solution might be integrated to achieve better performance.
//!
//! [^note]: Some of these claims are not yet fully implemented.

extern crate core;

use crate::{
    attachment_loader::{finish_loading_attachment_from_disk, start_loading_attachment_from_disk},
    debug::DebugTerrain,
    formats::TDFPlugin,
    render::{
        compute_pipelines::{TerrainComputeNode, TerrainComputePipelines},
        culling::{queue_terrain_culling_bind_group, CullingBindGroup},
        render_pipeline::TerrainPipelineConfig,
        shaders::add_shader,
        terrain_data::{initialize_terrain_data, TerrainData},
        terrain_view_data::TerrainViewConfigUniform,
        terrain_view_data::{
            extract_terrain_view_config, initialize_terrain_view_data, queue_terrain_view_config,
            TerrainViewData,
        },
    },
    terrain::{Terrain, TerrainComponents, TerrainConfig},
    terrain_data::{
        gpu_node_atlas::{
            extract_node_atlas, initialize_gpu_node_atlas, queue_node_atlas_updates, GpuNodeAtlas,
        },
        gpu_quadtree::{
            extract_quadtree, initialize_gpu_quadtree, queue_quadtree_update, GpuQuadtree,
        },
        node_atlas::{update_node_atlas, NodeAtlas},
        quadtree::{
            adjust_quadtree, compute_quadtree_request, update_height_under_viewer, Quadtree,
        },
    },
    terrain_view::{TerrainView, TerrainViewComponents, TerrainViewConfig},
};
use bevy::{
    prelude::*,
    render::{
        extract_component::ExtractComponentPlugin, main_graph::node::CAMERA_DRIVER,
        render_graph::RenderGraph, render_resource::*, RenderApp, RenderStage,
    },
};

pub mod attachment_loader;
pub mod debug;
pub mod formats;
pub mod preprocess;
pub mod render;
pub mod terrain;
pub mod terrain_data;
pub mod terrain_view;

pub mod prelude {
    //! `use bevy_terrain::prelude::*;` to import common components, bundles, and plugins.
    // #[doc(hidden)]
    pub use crate::{
        attachment_loader::AttachmentFromDiskLoader,
        debug::{camera::DebugCamera, TerrainDebugPlugin},
        preprocess::{config::load_node_config, BaseConfig, Preprocessor, TileConfig},
        render::render_pipeline::TerrainMaterialPlugin,
        terrain::{Terrain, TerrainConfig},
        terrain_data::{
            node_atlas::NodeAtlas, quadtree::Quadtree, AttachmentConfig, AttachmentFormat,
            FileFormat,
        },
        terrain_view::{TerrainView, TerrainViewComponents, TerrainViewConfig},
        TerrainBundle, TerrainPlugin,
    };
}

/// The components of a terrain.
///
/// Does not include loader(s) and a material.
#[derive(Bundle)]
pub struct TerrainBundle {
    #[bundle]
    visibility: VisibilityBundle,
    terrain: Terrain,
    node_atlas: NodeAtlas,
    config: TerrainConfig,
    transform: Transform,
    global_transform: GlobalTransform,
}

impl TerrainBundle {
    /// Creates a new terrain bundle from the config.
    pub fn new(config: TerrainConfig) -> Self {
        Self {
            visibility: default(),
            terrain: Terrain,
            node_atlas: NodeAtlas::from_config(&config),
            config,
            transform: default(),
            global_transform: default(),
        }
    }
}

/// The plugin for the terrain renderer.
pub struct TerrainPlugin {
    /// The number of terrain attachments.
    pub attachment_count: usize,
}

impl Default for TerrainPlugin {
    fn default() -> Self {
        Self {
            attachment_count: 2,
        }
    }
}

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        add_shader(app);

        app.add_plugin(TDFPlugin)
            .add_plugin(ExtractComponentPlugin::<Terrain>::default())
            .add_plugin(ExtractComponentPlugin::<TerrainView>::default())
            .init_resource::<TerrainViewComponents<Quadtree>>()
            .init_resource::<TerrainViewComponents<TerrainViewConfig>>()
            .add_system_to_stage(
                CoreStage::Last,
                finish_loading_attachment_from_disk.before(update_node_atlas),
            )
            .add_system_to_stage(
                CoreStage::Last,
                compute_quadtree_request.before(update_node_atlas),
            )
            .add_system_to_stage(CoreStage::Last, update_node_atlas)
            .add_system_to_stage(CoreStage::Last, adjust_quadtree.after(update_node_atlas))
            .add_system_to_stage(
                CoreStage::Last,
                start_loading_attachment_from_disk.after(update_node_atlas),
            )
            .add_system_to_stage(
                CoreStage::Last,
                update_height_under_viewer.after(adjust_quadtree),
            );

        let render_app = app
            .sub_app_mut(RenderApp)
            .insert_resource(TerrainPipelineConfig {
                attachment_count: self.attachment_count,
            })
            .init_resource::<TerrainComputePipelines>()
            .init_resource::<SpecializedComputePipelines<TerrainComputePipelines>>()
            .init_resource::<TerrainComponents<GpuNodeAtlas>>()
            .init_resource::<TerrainComponents<TerrainData>>()
            .init_resource::<TerrainViewComponents<GpuQuadtree>>()
            .init_resource::<TerrainViewComponents<TerrainViewData>>()
            .init_resource::<TerrainViewComponents<TerrainViewConfigUniform>>()
            .init_resource::<TerrainViewComponents<CullingBindGroup>>()
            .add_system_to_stage(RenderStage::Extract, extract_terrain_view_config)
            .add_system_to_stage(RenderStage::Extract, initialize_gpu_node_atlas)
            .add_system_to_stage(RenderStage::Extract, initialize_gpu_quadtree)
            .add_system_to_stage(
                RenderStage::Extract,
                initialize_terrain_data.after(initialize_gpu_node_atlas),
            )
            .add_system_to_stage(
                RenderStage::Extract,
                initialize_terrain_view_data.after(initialize_gpu_quadtree),
            )
            .add_system_to_stage(
                RenderStage::Extract,
                extract_node_atlas.after(initialize_gpu_node_atlas),
            )
            .add_system_to_stage(
                RenderStage::Extract,
                extract_quadtree.after(initialize_gpu_quadtree),
            )
            .add_system_to_stage(RenderStage::Queue, queue_quadtree_update)
            .add_system_to_stage(RenderStage::Queue, queue_node_atlas_updates)
            .add_system_to_stage(RenderStage::Queue, queue_terrain_culling_bind_group)
            .add_system_to_stage(RenderStage::Queue, queue_terrain_view_config);

        let compute_node = TerrainComputeNode::from_world(&mut render_app.world);

        let mut render_graph = render_app.world.resource_mut::<RenderGraph>();
        render_graph.add_node("terrain_compute", compute_node);

        render_graph
            .add_node_edge("terrain_compute", CAMERA_DRIVER)
            .unwrap();
    }
}
