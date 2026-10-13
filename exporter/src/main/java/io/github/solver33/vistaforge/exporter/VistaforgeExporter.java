package io.github.solver33.vistaforge.exporter;

import java.lang.reflect.Method;
import java.lang.reflect.Proxy;

import net.fabricmc.api.ClientModInitializer;

/**
 * Client entrypoint. Waits until the game has finished loading resources (the loading overlay is
 * gone, usually when the title screen appears), then exports the catalog once per launch.
 *
 * <p>Every failure is caught and logged: the exporter must never crash the game.
 */
public final class VistaforgeExporter implements ClientModInitializer {
    private static final String TICK_EVENTS = "net.fabricmc.fabric.api.client.event.lifecycle.v1.ClientTickEvents";
    private static final String EVENT = "net.fabricmc.fabric.api.event.Event";

    private boolean done;
    private Method getOverlay;

    @Override
    public void onInitializeClient() {
        try {
            hookEndClientTick();
            Exporter.LOG.info("[Vistaforge] Catalog export will run once the game has loaded");
        } catch (Throwable t) {
            Exporter.LOG.warn("[Vistaforge] Fabric API tick event unavailable, exporting blocks now without colours", t);
            done = true;
            Exporter.run(false);
        }
    }

    /** Registers on Fabric API's END_CLIENT_TICK through reflection, so Fabric API is not needed to compile. */
    private void hookEndClientTick() throws ReflectiveOperationException {
        ClassLoader loader = VistaforgeExporter.class.getClassLoader();
        Class<?> events = Class.forName(TICK_EVENTS, true, loader);
        Class<?> endTick = Class.forName(TICK_EVENTS + "$EndTick", false, loader);
        Object event = events.getField("END_CLIENT_TICK").get(null);
        Object listener = Proxy.newProxyInstance(loader, new Class<?>[] {endTick}, (proxy, method, args) -> {
            switch (method.getName()) {
                case "hashCode":
                    return System.identityHashCode(proxy);
                case "equals":
                    return proxy == args[0];
                case "toString":
                    return "VistaforgeExporter$EndTick";
                default:
                    onEndTick(args[0]);
                    return null;
            }
        });
        // Look the method up on the public Event class: the runtime subclass is package-private.
        Class.forName(EVENT, false, loader).getMethod("register", Object.class).invoke(event, listener);
    }

    private void onEndTick(Object client) {
        if (done) {
            return;
        }
        try {
            if (getOverlay == null) {
                Mc mc = new Mc(net.fabricmc.loader.api.FabricLoader.getInstance().getMappingResolver());
                getOverlay = mc.method("net.minecraft.class_310", "method_18506", "()Lnet/minecraft/class_4071;");
            }
            if (getOverlay.invoke(client) != null) {
                return; // resources are still loading
            }
        } catch (Throwable t) {
            Exporter.LOG.warn("[Vistaforge] Could not check the loading overlay, exporting now", t);
        }
        done = true;
        Exporter.run(true);
    }
}
