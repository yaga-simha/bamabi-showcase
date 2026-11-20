const std = @import("std");

fn resolveIncludeDir(b: *std.Build) std.Build.LazyPath {
    if (b.option([]const u8, "include-dir", "Path to directory containing bamabi.h")) |inc_opt| {
        if (inc_opt.len > 0) {
            return .{ .cwd_relative = inc_opt };
        }
    }
    if (b.graph.environ_map.get("BAM_INCLUDE_DIR")) |env_inc| {
        if (env_inc.len > 0) {
            return .{ .cwd_relative = env_inc };
        }
    }
    if (b.graph.environ_map.get("BAM_WIRE_DIR")) |wire_dir| {
        if (wire_dir.len > 0) {
            return .{ .cwd_relative = b.pathJoin(&.{ wire_dir, "include" }) };
        }
    }
    if (b.build_root.handle.access(b.graph.io, "../../../../bamabi-wire/include/bamabi.h", .{})) |_| {
        return b.path("../../../../bamabi-wire/include");
    } else |_| {}
    return b.path("../../../target/include");
}

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});
    const include_dir = resolveIncludeDir(b);

    const translate_c = b.addTranslateC(.{
        .root_source_file = include_dir.path(b, "bamabi.h"),
        .target = target,
        .optimize = optimize,
        .link_libc = true,
    });
    translate_c.addIncludePath(include_dir);
    const bam_c_mod = translate_c.createModule();

    const mod = b.createModule(.{
        .root_source_file = b.path("src/root.zig"),
        .target = target,
        .optimize = optimize,
        .link_libc = true,
    });
    mod.addImport("bam_c", bam_c_mod);

    const lib = b.addLibrary(.{
        .linkage = .dynamic,
        .name = "zig_mod",
        .root_module = mod,
    });

    b.installArtifact(lib);
}
