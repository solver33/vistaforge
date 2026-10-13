package io.github.solver33.vistaforge.exporter;

import java.io.BufferedWriter;
import java.io.IOException;
import java.io.Writer;
import java.nio.charset.StandardCharsets;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.time.Instant;
import java.time.temporal.ChronoUnit;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;

import net.fabricmc.loader.api.FabricLoader;
import net.fabricmc.loader.api.ModContainer;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/** Builds the catalog and writes it to {@code <game dir>/vistaforge/catalog.json}. */
final class Exporter {
    static final Logger LOG = LoggerFactory.getLogger("Vistaforge");
    static final String MOD_ID = "vistaforge_exporter";
    static final int FORMAT = 1;

    private Exporter() {
    }

    static void run(boolean withColors) {
        long start = System.nanoTime();
        try {
            FabricLoader loader = FabricLoader.getInstance();
            Mc mc = new Mc(loader.getMappingResolver());
            BlockCatalog catalog = BlockCatalog.read(mc);

            String colorError = null;
            int colorFailures = 0;
            if (withColors) {
                try {
                    TextureColors colors = new TextureColors(mc);
                    for (BlockCatalog.Entry entry : catalog.blocks) {
                        if (entry.defaultState == null) {
                            continue;
                        }
                        try {
                            colors.sample(entry);
                        } catch (Throwable t) {
                            entry.colorError = describe(t);
                            colorFailures++;
                        }
                    }
                } catch (Throwable t) {
                    colorError = describe(t);
                    LOG.warn("[Vistaforge] Texture colours skipped", t);
                }
            } else {
                colorError = "not sampled (Fabric API tick event unavailable)";
            }

            Path out = loader.getGameDir().resolve("vistaforge").resolve("catalog.json");
            write(loader, catalog, withColors && colorError == null, colorError, out);
            long ms = (System.nanoTime() - start) / 1_000_000;
            LOG.info("[Vistaforge] Exported {} blocks ({} states, {} colour failures) to {} in {} ms",
                    catalog.blocks.size(), catalog.stateCount, colorFailures, out.toAbsolutePath(), ms);
        } catch (Throwable t) {
            LOG.error("[Vistaforge] Catalog export failed", t);
        }
    }

    static String describe(Throwable t) {
        Throwable cause = t;
        while (cause.getCause() != null && cause.getCause() != cause) {
            cause = cause.getCause();
        }
        return cause.getClass().getName() + (cause.getMessage() == null ? "" : ": " + cause.getMessage());
    }

    private static void write(FabricLoader loader, BlockCatalog catalog, boolean colors, String colorError, Path out)
            throws IOException {
        Files.createDirectories(out.getParent());
        Path tmp = out.resolveSibling(out.getFileName() + ".tmp");
        try (Writer w = new BufferedWriter(Files.newBufferedWriter(tmp, StandardCharsets.UTF_8), 1 << 16)) {
            StringBuilder sb = new StringBuilder(1 << 12);
            sb.append("{\n");
            sb.append("\"format\": ").append(FORMAT).append(",\n");
            sb.append("\"exporter\": ");
            str(sb, version(loader, MOD_ID));
            sb.append(",\n\"exportedAt\": ");
            str(sb, Instant.now().truncatedTo(ChronoUnit.SECONDS).toString());
            sb.append(",\n\"runtimeNamespace\": ");
            str(sb, loader.getMappingResolver().getCurrentRuntimeNamespace());

            // Version of the mod behind each block namespace (null when no mod has that id).
            Map<String, String> mods = new TreeMap<>();
            mods.put("minecraft", version(loader, "minecraft"));
            for (BlockCatalog.Entry entry : catalog.blocks) {
                int colon = entry.id.indexOf(':');
                if (colon > 0) {
                    mods.computeIfAbsent(entry.id.substring(0, colon), ns -> version(loader, ns));
                }
            }
            mods.put("fabric-api", version(loader, "fabric-api"));
            sb.append(",\n\"mods\": {");
            boolean first = true;
            for (Map.Entry<String, String> mod : mods.entrySet()) {
                sb.append(first ? "" : ", ");
                first = false;
                str(sb, mod.getKey());
                sb.append(": ");
                str(sb, mod.getValue());
            }
            sb.append("},\n\"counts\": {\"blocks\": ").append(catalog.blocks.size())
                    .append(", \"states\": ").append(catalog.stateCount).append("},\n");
            sb.append("\"colors\": ").append(colors);
            if (colorError != null) {
                sb.append(",\n\"colorError\": ");
                str(sb, colorError);
            }
            sb.append(",\n\"classes\": {");
            first = true;
            for (Map.Entry<String, String> cls : catalog.classes.entrySet()) {
                sb.append(first ? "\n" : ",\n");
                first = false;
                str(sb, cls.getKey());
                sb.append(": ");
                str(sb, cls.getValue());
            }
            sb.append("\n},\n\"blocks\": {");
            w.write(sb.toString());

            first = true;
            for (BlockCatalog.Entry entry : catalog.blocks) {
                sb.setLength(0);
                sb.append(first ? "\n" : ",\n");
                first = false;
                block(sb, entry);
                w.write(sb.toString());
            }
            w.write("\n}\n}\n");
        }
        try {
            Files.move(tmp, out, StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE);
        } catch (AtomicMoveNotSupportedException e) {
            Files.move(tmp, out, StandardCopyOption.REPLACE_EXISTING);
        }
    }

    /** One block per line, so the file stays readable and diffable. */
    private static void block(StringBuilder sb, BlockCatalog.Entry e) {
        str(sb, e.id);
        sb.append(": {\"class\": ");
        str(sb, e.className);
        sb.append(", \"states\": ").append(e.stateCount);
        if (!e.properties.isEmpty()) {
            sb.append(", \"properties\": {");
            boolean first = true;
            for (Map.Entry<String, List<String>> p : e.properties.entrySet()) {
                sb.append(first ? "" : ", ");
                first = false;
                str(sb, p.getKey());
                sb.append(": [");
                for (int i = 0; i < p.getValue().size(); i++) {
                    sb.append(i == 0 ? "" : ", ");
                    str(sb, p.getValue().get(i));
                }
                sb.append(']');
            }
            sb.append("}, \"default\": {");
            first = true;
            for (Map.Entry<String, String> d : e.defaults.entrySet()) {
                sb.append(first ? "" : ", ");
                first = false;
                str(sb, d.getKey());
                sb.append(": ");
                str(sb, d.getValue());
            }
            sb.append('}');
        }
        if (e.mapColorId >= 0) {
            sb.append(", \"mapColor\": {\"id\": ").append(e.mapColorId).append(", \"rgb\": ");
            str(sb, hex(e.mapColorRgb));
            sb.append('}');
        }
        bool(sb, "air", e.air);
        bool(sb, "opaque", e.opaque);
        bool(sb, "liquid", e.liquid);
        face(sb, "top", e.top);
        face(sb, "side", e.side);
        face(sb, "particle", e.particle);
        if (e.error != null) {
            sb.append(", \"error\": ");
            str(sb, e.error);
        }
        if (e.colorError != null) {
            sb.append(", \"colorError\": ");
            str(sb, e.colorError);
        }
        sb.append('}');
    }

    private static void bool(StringBuilder sb, String name, Boolean value) {
        if (value != null) {
            sb.append(", \"").append(name).append("\": ").append(value);
        }
    }

    private static void face(StringBuilder sb, String name, TextureColors.Face f) {
        if (f == null) {
            return;
        }
        sb.append(", \"").append(name).append("\": {\"rgb\": ");
        str(sb, hex(f.rgb));
        sb.append(", \"coverage\": ").append(Math.round(f.coverage * 1000) / 1000.0);
        if (f.texture != null) {
            sb.append(", \"texture\": ");
            str(sb, f.texture);
        }
        if (f.tintIndex >= 0) {
            sb.append(", \"tintIndex\": ").append(f.tintIndex);
            if (f.tint != -1) {
                sb.append(", \"tint\": ");
                str(sb, hex(f.tint));
            }
        }
        sb.append('}');
    }

    private static String hex(int rgb) {
        return String.format("%06x", rgb & 0xFFFFFF);
    }

    private static String version(FabricLoader loader, String modId) {
        try {
            return loader.getModContainer(modId)
                    .map(ModContainer::getMetadata)
                    .map(m -> m.getVersion().getFriendlyString())
                    .orElse(null);
        } catch (Throwable t) {
            return null;
        }
    }

    static void str(StringBuilder sb, String s) {
        if (s == null) {
            sb.append("null");
            return;
        }
        sb.append('"');
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            switch (c) {
                case '"': sb.append("\\\""); break;
                case '\\': sb.append("\\\\"); break;
                case '\n': sb.append("\\n"); break;
                case '\r': sb.append("\\r"); break;
                case '\t': sb.append("\\t"); break;
                default:
                    if (c < 0x20) {
                        sb.append(String.format("\\u%04x", (int) c));
                    } else {
                        sb.append(c);
                    }
            }
        }
        sb.append('"');
    }
}
