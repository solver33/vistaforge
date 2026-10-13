package io.github.solver33.vistaforge.exporter;

import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.util.ArrayList;
import java.util.Collection;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/** Reads every registered block, its block-state properties and a few flags from the live registry. */
final class BlockCatalog {
    private static final String REGISTRIES = "net.minecraft.class_7923";
    private static final String REGISTRY = "net.minecraft.class_2378";
    private static final String ABSTRACT_BLOCK = "net.minecraft.class_4970";
    private static final String BLOCK = "net.minecraft.class_2248";
    private static final String ABSTRACT_BLOCK_STATE = "net.minecraft.class_4970$class_4971";
    private static final String STATE = "net.minecraft.class_2688";
    private static final String STATE_MANAGER = "net.minecraft.class_2689";
    private static final String PROPERTY = "net.minecraft.class_2769";
    private static final String MAP_COLOR = "net.minecraft.class_3620";

    final List<Entry> blocks = new ArrayList<>();
    /** Runtime class name to its superclass name, for every class in a block's hierarchy. */
    final Map<String, String> classes = new LinkedHashMap<>();
    long stateCount;

    static final class Entry {
        String id;
        Object defaultState;
        String className;
        int stateCount;
        final Map<String, List<String>> properties = new LinkedHashMap<>();
        final Map<String, String> defaults = new LinkedHashMap<>();
        int mapColorId = -1;
        int mapColorRgb = -1;
        Boolean air;
        Boolean opaque;
        Boolean liquid;
        String error;
        TextureColors.Face top;
        TextureColors.Face side;
        TextureColors.Face particle;
        String colorError;
    }

    static BlockCatalog read(Mc mc) throws ReflectiveOperationException {
        Object registry = mc.field(REGISTRIES, "field_41175", "Lnet/minecraft/class_7922;").get(null);
        Method getId = mc.method(REGISTRY, "method_10221", "(Ljava/lang/Object;)Lnet/minecraft/class_2960;");
        Method getStateManager = mc.method(BLOCK, "method_9595", "()Lnet/minecraft/class_2689;");
        Method getDefaultState = mc.method(BLOCK, "method_9564", "()Lnet/minecraft/class_2680;");
        Method getStates = mc.method(STATE_MANAGER, "method_11662", "()Lcom/google/common/collect/ImmutableList;");
        Method getProperties = mc.method(STATE_MANAGER, "method_11659", "()Ljava/util/Collection;");
        Method propertyName = mc.method(PROPERTY, "method_11899", "()Ljava/lang/String;");
        Method propertyValues = mc.method(PROPERTY, "method_11898", "()Ljava/util/Collection;");
        Method valueName = mc.method(PROPERTY, "method_11901", "(Ljava/lang/Comparable;)Ljava/lang/String;");
        Method stateGet = mc.method(STATE, "method_11654", "(Lnet/minecraft/class_2769;)Ljava/lang/Comparable;");

        // Nice to have; the catalog is still useful without them.
        Method mapColor = optional(() -> mc.method(ABSTRACT_BLOCK, "method_26403", "()Lnet/minecraft/class_3620;"));
        Field mapColorRgb = optional(() -> mc.field(MAP_COLOR, "field_16011", "I"));
        Field mapColorId = optional(() -> mc.field(MAP_COLOR, "field_16021", "I"));
        Method isAir = optional(() -> mc.method(ABSTRACT_BLOCK_STATE, "method_26215", "()Z"));
        Method isOpaque = optional(() -> mc.method(ABSTRACT_BLOCK_STATE, "method_26225", "()Z"));
        Method isLiquid = optional(() -> mc.method(ABSTRACT_BLOCK_STATE, "method_51176", "()Z"));

        BlockCatalog catalog = new BlockCatalog();
        for (Object block : (Iterable<?>) registry) {
            Entry entry = new Entry();
            try {
                entry.id = String.valueOf(getId.invoke(registry, block));
                entry.className = block.getClass().getName();
                catalog.addClassChain(block.getClass());

                Object stateManager = getStateManager.invoke(block);
                entry.stateCount = ((List<?>) getStates.invoke(stateManager)).size();
                entry.defaultState = getDefaultState.invoke(block);
                for (Object property : (Collection<?>) getProperties.invoke(stateManager)) {
                    String name = (String) propertyName.invoke(property);
                    List<String> values = new ArrayList<>();
                    for (Object value : (Collection<?>) propertyValues.invoke(property)) {
                        values.add((String) valueName.invoke(property, value));
                    }
                    entry.properties.put(name, values);
                    entry.defaults.put(name, (String) valueName.invoke(property, stateGet.invoke(entry.defaultState, property)));
                }

                if (mapColor != null && mapColorRgb != null && mapColorId != null) {
                    Object color = mapColor.invoke(block);
                    if (color != null) {
                        entry.mapColorId = mapColorId.getInt(color);
                        entry.mapColorRgb = mapColorRgb.getInt(color);
                    }
                }
                entry.air = flag(isAir, entry.defaultState);
                entry.opaque = flag(isOpaque, entry.defaultState);
                entry.liquid = flag(isLiquid, entry.defaultState);
            } catch (Throwable t) {
                entry.error = Exporter.describe(t);
            }
            if (entry.id == null) {
                entry.id = "unknown:" + catalog.blocks.size();
            }
            catalog.stateCount += entry.stateCount;
            catalog.blocks.add(entry);
        }
        return catalog;
    }

    private void addClassChain(Class<?> cls) {
        for (Class<?> c = cls; c != null && c != Object.class; c = c.getSuperclass()) {
            if (classes.containsKey(c.getName())) {
                return;
            }
            Class<?> parent = c.getSuperclass();
            classes.put(c.getName(), parent == null ? null : parent.getName());
        }
    }

    private static Boolean flag(Method method, Object state) {
        if (method == null) {
            return null;
        }
        try {
            return (Boolean) method.invoke(state);
        } catch (Throwable t) {
            return null;
        }
    }

    interface Lookup<T> {
        T get() throws ReflectiveOperationException;
    }

    static <T> T optional(Lookup<T> lookup) {
        try {
            return lookup.get();
        } catch (Throwable t) {
            Exporter.LOG.warn("[Vistaforge] Optional lookup failed: " + Exporter.describe(t));
            return null;
        }
    }
}
