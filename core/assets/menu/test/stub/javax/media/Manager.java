package javax.media;
public class Manager {
    public static Player createPlayer(MediaLocator loc) {
        Player.lastUri = loc.toExternalForm();
        return new Player();
    }
}
