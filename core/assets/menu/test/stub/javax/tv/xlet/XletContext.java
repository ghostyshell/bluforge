package javax.tv.xlet;
public interface XletContext {
    void notifyDestroyed();
    Object getXletProperty(String key);
}
