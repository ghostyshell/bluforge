package javax.media;
public class Player {
    public static String lastUri;
    private ControllerListener listener;
    public void addControllerListener(ControllerListener l) { listener = l; }
    public void start() {}
    public void stop() {}
    public void deallocate() {}
    public void close() {}
    public void fireEnd() { if (listener != null) listener.controllerUpdate(new EndOfMediaEvent(this)); }
}
