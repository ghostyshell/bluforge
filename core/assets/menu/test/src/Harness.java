import java.awt.Frame;
import java.awt.Component;
import java.awt.Rectangle;
import java.awt.Robot;
import java.awt.image.BufferedImage;
import java.awt.event.KeyEvent;
import java.io.File;
import java.io.FileOutputStream;
import java.util.Arrays;

import javax.imageio.ImageIO;
import javax.tv.xlet.XletContext;
import javax.media.Player;

/**
 * Standalone preview/verification harness for the Keepsake Atelier BD-J menu
 * Xlet. It replaces the BD-J platform with stubs, runs the real Menu class,
 * drives it with key events, captures the window, and prints what the menu
 * asked to play. Not part of the app: libbluray's macOS BD-J cannot start AWT
 * from its calling thread, so this is how the asset is verified.
 */
public class Harness {

    static class Ctx implements XletContext {
        public void notifyDestroyed() {}
        public Object getXletProperty(String key) { return null; }
    }

    static Frame visibleFrame() {
        for (Frame f : Frame.getFrames()) {
            if (f.isVisible()) {
                return f;
            }
        }
        return null;
    }

    static void key(Component c, int code) {
        long now = System.currentTimeMillis();
        c.dispatchEvent(new KeyEvent(c, KeyEvent.KEY_PRESSED, now, 0, code,
            KeyEvent.CHAR_UNDEFINED));
    }

    static void shot(Frame f, String path) throws Exception {
        f.toFront();
        f.requestFocus();
        pause(250);
        Rectangle r = f.getBounds();
        Robot robot = new Robot();
        BufferedImage img = robot.createScreenCapture(r);
        ImageIO.write(img, "png", new File(path));
        System.out.println("HARNESS_SHOT " + path + " " + r.width + "x" + r.height);
    }

    static void pause(long ms) {
        try {
            Thread.sleep(ms);
        } catch (InterruptedException ignored) {
        }
    }

    public static void main(String[] args) throws Exception {
        String shotDir = args.length > 0 ? args[0] : "/tmp";
        Menu menu = new Menu();
        menu.initXlet(new Ctx());
        System.out.println("HARNESS_START");
        menu.startXlet();
        pause(1800);

        Frame f = visibleFrame();
        if (f == null) {
            System.out.println("HARNESS_NO_FRAME");
            System.exit(2);
        }
        f.setAlwaysOnTop(true);
        f.setLocation(0, 0);
        f.setSize(960, 540);
        f.toFront();
        f.requestFocus();
        Component canvas = f.getComponent(0);
        pause(800);
        shot(f, shotDir + "/menu-1.png");

        key(canvas, KeyEvent.VK_DOWN);
        pause(400);
        shot(f, shotDir + "/menu-2.png");

        key(canvas, KeyEvent.VK_ENTER);
        pause(600);
        System.out.println("HARNESS_PLAYER " + Player.lastUri);

        // Simulate the title ending: the menu must come back.
        menu.controllerUpdate(new javax.media.EndOfMediaEvent(new Object()));
        pause(700);
        Frame again = visibleFrame();
        System.out.println("HARNESS_RETURNED " + (again != null));
        if (again != null) {
            shot(again, shotDir + "/menu-3.png");
        }

        System.out.println("HARNESS_OK " + Arrays.toString(args));
        System.exit(0);
    }
}
