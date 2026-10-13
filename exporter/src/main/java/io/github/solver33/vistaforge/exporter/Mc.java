package io.github.solver33.vistaforge.exporter;

import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.util.ArrayList;
import java.util.List;

import net.fabricmc.loader.api.MappingResolver;

/**
 * Looks up Minecraft classes and members by their intermediary names.
 *
 * <p>The exporter does not compile against Minecraft. In a normal (production) Fabric install the
 * runtime names are the intermediary names, so the resolver is an identity mapping; in a development
 * environment it maps them to Yarn names. Names were checked against Yarn 1.20.1.
 */
final class Mc {
    private static final String INTERMEDIARY = "intermediary";

    private final MappingResolver resolver;
    private final ClassLoader loader = Mc.class.getClassLoader();

    Mc(MappingResolver resolver) {
        this.resolver = resolver;
    }

    /** @param name intermediary binary name, e.g. {@code net.minecraft.class_4970$class_4971} */
    Class<?> cls(String name) throws ClassNotFoundException {
        String mapped = resolver == null ? name : resolver.mapClassName(INTERMEDIARY, name);
        return Class.forName(mapped, false, loader);
    }

    /**
     * @param owner intermediary owner class name
     * @param name intermediary method name
     * @param desc intermediary method descriptor; the parameter types are resolved from it
     */
    Method method(String owner, String name, String desc) throws ReflectiveOperationException {
        Class<?> cls = cls(owner);
        String mapped = resolver == null ? name : resolver.mapMethodName(INTERMEDIARY, owner, name, desc);
        Class<?>[] params = parameterTypes(desc);
        Method method;
        try {
            method = cls.getMethod(mapped, params);
        } catch (NoSuchMethodException e) {
            method = findDeclaredMethod(cls, mapped, params);
        }
        trySetAccessible(method);
        return method;
    }

    Field field(String owner, String name, String desc) throws ReflectiveOperationException {
        Class<?> cls = cls(owner);
        String mapped = resolver == null ? name : resolver.mapFieldName(INTERMEDIARY, owner, name, desc);
        for (Class<?> c = cls; c != null; c = c.getSuperclass()) {
            try {
                Field field = c.getDeclaredField(mapped);
                trySetAccessible(field);
                return field;
            } catch (NoSuchFieldException ignored) {
                // keep looking in the superclass
            }
        }
        throw new NoSuchFieldException(owner + "." + name + " (" + mapped + ")");
    }

    private static Method findDeclaredMethod(Class<?> cls, String name, Class<?>[] params) throws NoSuchMethodException {
        for (Class<?> c = cls; c != null; c = c.getSuperclass()) {
            try {
                return c.getDeclaredMethod(name, params);
            } catch (NoSuchMethodException ignored) {
                // keep looking in the superclass
            }
        }
        throw new NoSuchMethodException(cls.getName() + "." + name);
    }

    private static void trySetAccessible(java.lang.reflect.AccessibleObject member) {
        try {
            member.setAccessible(true);
        } catch (RuntimeException ignored) {
            // public members still work without it
        }
    }

    private Class<?>[] parameterTypes(String desc) throws ClassNotFoundException {
        List<Class<?>> types = new ArrayList<>();
        int i = 1; // skip '('
        while (desc.charAt(i) != ')') {
            int start = i;
            while (desc.charAt(i) == '[') {
                i++;
            }
            if (desc.charAt(i) == 'L') {
                i = desc.indexOf(';', i);
            }
            i++;
            types.add(type(desc.substring(start, i)));
        }
        return types.toArray(new Class<?>[0]);
    }

    private Class<?> type(String desc) throws ClassNotFoundException {
        switch (desc.charAt(0)) {
            case 'Z': return boolean.class;
            case 'B': return byte.class;
            case 'C': return char.class;
            case 'S': return short.class;
            case 'I': return int.class;
            case 'J': return long.class;
            case 'F': return float.class;
            case 'D': return double.class;
            case 'L': return cls(desc.substring(1, desc.length() - 1).replace('/', '.'));
            case '[': return java.lang.reflect.Array.newInstance(type(desc.substring(1)), 0).getClass();
            default: throw new IllegalArgumentException("Bad descriptor: " + desc);
        }
    }
}
