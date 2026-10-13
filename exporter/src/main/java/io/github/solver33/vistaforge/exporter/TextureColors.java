package io.github.solver33.vistaforge.exporter;

import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.util.ArrayList;
import java.util.IdentityHashMap;
import java.util.List;
import java.util.Map;

/**
 * Averages the textures of each block's default-state model, for the map preview.
 *
 * <p>Needs the client to have finished loading resources (models baked, block atlas stitched).
 * Colours are untinted; when a face is tinted (grass, leaves) the tint index and the block's
 * default tint colour are recorded separately.
 */
final class TextureColors {
    private static final String CLIENT = "net.minecraft.class_310";
    private static final String RENDER_MANAGER = "net.minecraft.class_776";
    private static final String BLOCK_MODELS = "net.minecraft.class_773";
    private static final String BAKED_MODEL = "net.minecraft.class_1087";
    private static final String BAKED_QUAD = "net.minecraft.class_777";
    private static final String SPRITE = "net.minecraft.class_1058";
    private static final String SPRITE_CONTENTS = "net.minecraft.class_7764";
    private static final String NATIVE_IMAGE = "net.minecraft.class_1011";
    private static final String MC_BLOCK_COLORS = "net.minecraft.class_324";
    private static final String DIRECTION = "net.minecraft.class_2350";
    private static final String RANDOM = "net.minecraft.class_5819";
    private static final int MAX_SAMPLES_PER_SIDE = 32;

    /** Average colour of the quads facing one way. */
    static final class Face {
        int rgb;
        /** Share of opaque pixels, 0..1 (leaves and plants are below 1). */
        double coverage;
        String texture;
        int tintIndex = -1;
        int tint = -1;
    }

    private static final class SpriteStats {
        int rgb;
        double coverage;
        String id;
    }

    private final Object models;
    private final Object blockColors;
    private final Method getModel;
    private final Method getQuads;
    private final Method getParticle;
    private final Method quadSprite;
    private final Method quadFace;
    private final Method quadTint;
    private final Method spriteContents;
    private final Field contentsId;
    private final Field contentsWidth;
    private final Field contentsHeight;
    private final Field contentsImage;
    private final Method imageGetColor;
    private final Method blockColorsGet;
    private final Method randomCreate;
    private final Object[] directions;
    private final Object up;
    private final Object[] horizontal;
    private final Map<Object, SpriteStats> spriteCache = new IdentityHashMap<>();

    TextureColors(Mc mc) throws ReflectiveOperationException {
        Object client = mc.method(CLIENT, "method_1551", "()Lnet/minecraft/class_310;").invoke(null);
        Object renderManager = mc.method(CLIENT, "method_1541", "()Lnet/minecraft/class_776;").invoke(client);
        models = mc.method(RENDER_MANAGER, "method_3351", "()Lnet/minecraft/class_773;").invoke(renderManager);
        blockColors = mc.method(CLIENT, "method_1505", "()Lnet/minecraft/class_324;").invoke(client);
        getModel = mc.method(BLOCK_MODELS, "method_3335", "(Lnet/minecraft/class_2680;)Lnet/minecraft/class_1087;");
        getQuads = mc.method(BAKED_MODEL, "method_4707",
                "(Lnet/minecraft/class_2680;Lnet/minecraft/class_2350;Lnet/minecraft/class_5819;)Ljava/util/List;");
        getParticle = mc.method(BAKED_MODEL, "method_4711", "()Lnet/minecraft/class_1058;");
        quadSprite = mc.method(BAKED_QUAD, "method_35788", "()Lnet/minecraft/class_1058;");
        quadFace = mc.method(BAKED_QUAD, "method_3358", "()Lnet/minecraft/class_2350;");
        quadTint = mc.method(BAKED_QUAD, "method_3359", "()I");
        spriteContents = mc.method(SPRITE, "method_45851", "()Lnet/minecraft/class_7764;");
        contentsId = mc.field(SPRITE_CONTENTS, "field_40536", "Lnet/minecraft/class_2960;");
        contentsWidth = mc.field(SPRITE_CONTENTS, "field_40537", "I");
        contentsHeight = mc.field(SPRITE_CONTENTS, "field_40538", "I");
        contentsImage = mc.field(SPRITE_CONTENTS, "field_40539", "Lnet/minecraft/class_1011;");
        imageGetColor = mc.method(NATIVE_IMAGE, "method_4315", "(II)I");
        blockColorsGet = mc.method(MC_BLOCK_COLORS, "method_1697",
                "(Lnet/minecraft/class_2680;Lnet/minecraft/class_1920;Lnet/minecraft/class_2338;I)I");
        randomCreate = mc.method(RANDOM, "method_43049", "(J)Lnet/minecraft/class_5819;");

        // Direction is an enum declared DOWN, UP, NORTH, SOUTH, WEST, EAST; toString() gives the lowercase name.
        directions = mc.cls(DIRECTION).getEnumConstants();
        up = direction("up", 1);
        horizontal = new Object[] {direction("north", 2), direction("south", 3), direction("west", 4), direction("east", 5)};
    }

    private Object direction(String name, int ordinal) {
        for (Object d : directions) {
            if (name.equals(String.valueOf(d))) {
                return d;
            }
        }
        return directions[ordinal];
    }

    /** Fills top, side and particle colours of one block. */
    void sample(BlockCatalog.Entry entry) throws ReflectiveOperationException {
        Object state = entry.defaultState;
        Object model = getModel.invoke(models, state);
        if (model == null) {
            return;
        }

        // Quads grouped by the way they face: culled quads per direction plus unculled ones by their own face.
        Map<Object, List<Object>> byFace = new IdentityHashMap<>();
        List<Object> all = new ArrayList<>();
        for (Object direction : directions) {
            for (Object quad : (List<?>) getQuads.invoke(model, state, direction, randomCreate.invoke(null, 42L))) {
                byFace.computeIfAbsent(direction, k -> new ArrayList<>()).add(quad);
                all.add(quad);
            }
        }
        for (Object quad : (List<?>) getQuads.invoke(model, state, null, randomCreate.invoke(null, 42L))) {
            byFace.computeIfAbsent(quadFace.invoke(quad), k -> new ArrayList<>()).add(quad);
            all.add(quad);
        }

        List<Object> sides = new ArrayList<>();
        for (Object direction : horizontal) {
            sides.addAll(byFace.getOrDefault(direction, List.of()));
        }

        Object particleSprite = getParticle.invoke(model);
        entry.particle = particleSprite == null ? null : face(state, List.of(), particleSprite);
        entry.top = face(state, byFace.getOrDefault(up, all), particleSprite);
        entry.side = face(state, sides.isEmpty() ? all : sides, particleSprite);
    }

    private Face face(Object state, List<Object> quads, Object fallbackSprite) throws ReflectiveOperationException {
        Face face = new Face();
        if (quads.isEmpty()) {
            if (fallbackSprite == null) {
                return null;
            }
            SpriteStats stats = stats(fallbackSprite);
            face.rgb = stats.rgb;
            face.coverage = stats.coverage;
            face.texture = stats.id;
            return face;
        }

        double r = 0, g = 0, b = 0, weight = 0, coverage = 0;
        for (Object quad : quads) {
            SpriteStats stats = stats(quadSprite.invoke(quad));
            double w = Math.max(stats.coverage, 1e-3);
            r += w * ((stats.rgb >> 16) & 0xFF);
            g += w * ((stats.rgb >> 8) & 0xFF);
            b += w * (stats.rgb & 0xFF);
            weight += w;
            coverage += stats.coverage;
            if (face.texture == null) {
                face.texture = stats.id;
            }
            int tintIndex = (Integer) quadTint.invoke(quad);
            if (face.tintIndex < 0 && tintIndex >= 0) {
                face.tintIndex = tintIndex;
            }
        }
        face.rgb = ((int) Math.round(r / weight) << 16) | ((int) Math.round(g / weight) << 8) | (int) Math.round(b / weight);
        face.coverage = coverage / quads.size();
        if (face.tintIndex >= 0) {
            try {
                // No world or position: providers fall back to their default colour (e.g. plains grass).
                face.tint = (Integer) blockColorsGet.invoke(blockColors, state, null, null, face.tintIndex);
            } catch (Throwable t) {
                face.tint = -1;
            }
        }
        return face;
    }

    private SpriteStats stats(Object sprite) throws ReflectiveOperationException {
        SpriteStats cached = spriteCache.get(sprite);
        if (cached != null) {
            return cached;
        }
        SpriteStats stats = new SpriteStats();
        Object contents = spriteContents.invoke(sprite);
        stats.id = String.valueOf(contentsId.get(contents));
        Object image = contentsImage.get(contents);
        // Width and height are one animation frame; the image may hold several frames below it.
        int width = contentsWidth.getInt(contents);
        int height = contentsHeight.getInt(contents);
        int stepX = Math.max(1, width / MAX_SAMPLES_PER_SIDE);
        int stepY = Math.max(1, height / MAX_SAMPLES_PER_SIDE);
        long r = 0, g = 0, b = 0, opaque = 0, total = 0;
        for (int y = 0; y < height; y += stepY) {
            for (int x = 0; x < width; x += stepX) {
                int abgr = (Integer) imageGetColor.invoke(image, x, y);
                total++;
                if ((abgr >>> 24) > 16) {
                    r += abgr & 0xFF;
                    g += (abgr >> 8) & 0xFF;
                    b += (abgr >> 16) & 0xFF;
                    opaque++;
                }
            }
        }
        if (opaque > 0) {
            stats.rgb = (int) (r / opaque) << 16 | (int) (g / opaque) << 8 | (int) (b / opaque);
        }
        stats.coverage = total == 0 ? 0 : (double) opaque / total;
        spriteCache.put(sprite, stats);
        return stats;
    }
}
