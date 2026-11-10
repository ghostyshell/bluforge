package javax.tv.xlet;
public interface Xlet {
    void initXlet(XletContext c) throws XletStateChangeException;
    void startXlet() throws XletStateChangeException;
    void pauseXlet();
    void destroyXlet(boolean force) throws XletStateChangeException;
}
