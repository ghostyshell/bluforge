package javax.media;
public class MediaLocator {
    private final String s;
    public MediaLocator(String s) { this.s = s; }
    public String getProtocol() { int c = s.indexOf(':'); return c < 0 ? "" : s.substring(0, c); }
    public String getRemainder() { int c = s.indexOf(':'); return c < 0 ? "" : s.substring(c + 1); }
    public String toExternalForm() { return s; }
    public String toString() { return s; }
}
