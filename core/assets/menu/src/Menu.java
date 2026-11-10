/*
 * Keepsake Atelier - authored Blu-ray menu Xlet.
 *
 * A single generic JAR drives every authored disc: the title list is read at
 * runtime from /BDMV/JAR/menu.txt (a plain text file the authoring tool
 * writes next to the JAR), and the optional /BDMV/JAR/menu.png is used as the
 * background. No per-disc Java compilation is needed.
 *
 * menu.txt:
 *   line 1: "<heading>|<disc label>"
 *   line n: "<label>|<playlist number>"
 *   optional: "bg=<file name inside BDMV/JAR>"
 *   optional: "accent=<rrggbb>"
 *   '#' starts a comment, blank lines are ignored.
 *
 * Keys: Up/Down (also Left/Right), Enter/Space/Play to start, 1-9 to jump.
 * Mouse clicks select a row. Playback starts through javax.media with the
 * libbluray/BD-J "bd://" locator, and the menu returns when the title ends.
 */

import java.awt.Canvas;
import java.awt.Color;
import java.awt.Font;
import java.awt.Frame;
import java.awt.Graphics;
import java.awt.Image;
import java.awt.MediaTracker;
import java.awt.Toolkit;
import java.awt.event.KeyAdapter;
import java.awt.event.KeyEvent;
import java.awt.event.MouseAdapter;
import java.awt.event.MouseEvent;
import java.io.BufferedReader;
import java.io.File;
import java.io.FileInputStream;
import java.io.InputStreamReader;
import java.util.ArrayList;
import java.util.List;

import javax.media.ControllerEvent;
import javax.media.ControllerListener;
import javax.media.EndOfMediaEvent;
import javax.media.Manager;
import javax.media.MediaLocator;
import javax.media.Player;
import javax.tv.xlet.Xlet;
import javax.tv.xlet.XletContext;
import javax.tv.xlet.XletStateChangeException;

public class Menu implements Xlet, ControllerListener {

    private static final String JAR_DIR = "/BDMV/JAR/";

    /**
     * Where menu.txt and the background live. BD-J mounts the disc at /, so
     * that is the default. The property is a test/preview hook: the standalone
     * harness has no disc filesystem, and a real player simply denies it.
     */
    private static String jarDir() {
        try {
            String root = System.getProperty("keepsake.menu.root");
            if (root != null && root.length() > 0) {
                return root.endsWith("/") ? root : root + "/";
            }
        } catch (Throwable ignored) {
        }
        return JAR_DIR;
    }

    private XletContext context;
    private Frame frame;
    private MenuPanel panel;
    private Player player;
    private volatile boolean playing = false;

    private String heading = "Keepsake Atelier";
    private String discLabel = "";
    private String backgroundName = "menu.png";
    private Color accent = new Color(0xE8C36A);

    private final List<String> labels = new ArrayList<String>();
    private final List<Integer> playlists = new ArrayList<Integer>();
    private int selected = 0;

    public void initXlet(XletContext context) throws XletStateChangeException {
        this.context = context;
        loadMenu();
        System.out.println("KEEPSAKE_MENU_INIT entries=" + labels.size());
    }

    public void startXlet() throws XletStateChangeException {
        System.out.println("KEEPSAKE_MENU_START");
        frame = new Frame();
        frame.setUndecorated(true);
        frame.setBackground(Color.BLACK);
        panel = new MenuPanel();
        frame.add(panel);
        frame.pack();
        frame.setVisible(true);

        panel.addKeyListener(new KeyAdapter() {
            public void keyPressed(KeyEvent e) {
                onKey(e.getKeyCode());
            }
        });
        panel.addMouseListener(new MouseAdapter() {
            public void mouseClicked(MouseEvent e) {
                int i = rowAt(e.getY());
                if (i >= 0 && i < labels.size()) {
                    selected = i;
                    panel.repaint();
                    play(playlists.get(i));
                }
            }
        });
        frame.requestFocus();
        panel.requestFocus();
    }

    public void pauseXlet() {
    }

    public void destroyXlet(boolean force) throws XletStateChangeException {
        stopPlayer();
        if (frame != null) {
            frame.dispose();
            frame = null;
        }
    }

    /* ------------------------------------------------------------------ */

    private void onKey(int key) {
        if (labels.isEmpty()) {
            return;
        }
        if (key == KeyEvent.VK_UP || key == KeyEvent.VK_LEFT) {
            selected = (selected + labels.size() - 1) % labels.size();
            panel.repaint();
        } else if (key == KeyEvent.VK_DOWN || key == KeyEvent.VK_RIGHT) {
            selected = (selected + 1) % labels.size();
            panel.repaint();
        } else if (key == KeyEvent.VK_ENTER || key == KeyEvent.VK_SPACE) {
            play(playlists.get(selected));
        } else if (key >= KeyEvent.VK_1 && key <= KeyEvent.VK_9) {
            int i = key - KeyEvent.VK_1;
            if (i < labels.size()) {
                selected = i;
                panel.repaint();
                play(playlists.get(i));
            }
        }
    }

    private void play(int playlist) {
        stopPlayer();
        try {
            // libbluray (and BD-J players) resolve bd:// through the BD
            // protocol data source; org.bluray.net.BDLocator is not a
            // javax.media.MediaLocator, so the URL form is what we pass.
            MediaLocator locator =
                new MediaLocator("bd://1.PLAYLIST:" + pad(playlist));
            System.out.println("KEEPSAKE_MENU_PLAY " + locator);
            player = Manager.createPlayer(locator);
            player.addControllerListener(this);
            player.start();
            playing = true;
            if (frame != null) {
                frame.setVisible(false);
            }
        } catch (Throwable t) {
            System.out.println("KEEPSAKE_MENU_PLAY_FAILED " + t);
            t.printStackTrace();
            playing = false;
            stopPlayer();
            showMenu();
        }
    }

    private static String pad(int n) {
        String s = Integer.toString(n);
        while (s.length() < 5) {
            s = "0" + s;
        }
        return s;
    }

    private void stopPlayer() {
        if (player != null) {
            try {
                player.stop();
            } catch (Throwable ignored) {
            }
            try {
                player.deallocate();
            } catch (Throwable ignored) {
            }
            try {
                player.close();
            } catch (Throwable ignored) {
            }
            player = null;
        }
    }

    public void controllerUpdate(ControllerEvent event) {
        if (event instanceof EndOfMediaEvent) {
            System.out.println("KEEPSAKE_MENU_END");
            playing = false;
            stopPlayer();
            // Playback ran in this JVM: bring the menu back.
            showMenu();
        }
    }

    private void showMenu() {
        if (frame == null) {
            return;
        }
        frame.setVisible(true);
        frame.requestFocus();
        if (panel != null) {
            panel.requestFocus();
            panel.repaint();
        }
    }

    /* ------------------------------------------------------------------ */

    private void loadMenu() {
        BufferedReader in = null;
        try {
            in = new BufferedReader(new InputStreamReader(
                new FileInputStream(new File(jarDir() + "menu.txt")), "UTF-8"));
            String line;
            boolean first = true;
            while ((line = in.readLine()) != null) {
                line = line.trim();
                if (line.length() == 0 || line.charAt(0) == '#') {
                    continue;
                }
                if (line.startsWith("bg=")) {
                    backgroundName = line.substring(3).trim();
                    continue;
                }
                if (line.startsWith("accent=")) {
                    try {
                        accent = new Color(Integer.parseInt(
                            line.substring(7).trim(), 16));
                    } catch (Throwable ignored) {
                    }
                    continue;
                }
                int bar = line.lastIndexOf('|');
                if (bar <= 0) {
                    continue;
                }
                String label = line.substring(0, bar).trim();
                String tail = line.substring(bar + 1).trim();
                if (first) {
                    first = false;
                    heading = label.length() > 0 ? label : heading;
                    discLabel = tail;
                    continue;
                }
                try {
                    playlists.add(Integer.valueOf(Integer.parseInt(tail)));
                    labels.add(label);
                } catch (NumberFormatException ignored) {
                }
            }
        } catch (Throwable t) {
            System.out.println("KEEPSAKE_MENU_NO_LIST " + t);
        } finally {
            if (in != null) {
                try {
                    in.close();
                } catch (Throwable ignored) {
                }
            }
        }
        if (labels.isEmpty()) {
            labels.add("Play title 1");
            playlists.add(Integer.valueOf(0));
        }
        if (selected >= labels.size()) {
            selected = 0;
        }
    }

    private Image loadBackground() {
        try {
            Image image = Toolkit.getDefaultToolkit().getImage(
                jarDir() + backgroundName);
            MediaTracker tracker = new MediaTracker(panel);
            tracker.addImage(image, 0);
            tracker.waitForID(0, 3000);
            if (image.getWidth(null) > 0) {
                return image;
            }
        } catch (Throwable t) {
            System.out.println("KEEPSAKE_MENU_NO_BG " + t);
        }
        return null;
    }

    private int width() {
        int w = panel != null
            ? (panel.getWidth() > 0 ? panel.getWidth() : 1920)
            : 1920;
        return w;
    }

    private int height() {
        int h = panel != null
            ? (panel.getHeight() > 0 ? panel.getHeight() : 1080)
            : 1080;
        return h;
    }

    private int rowY(int i) {
        int h = height();
        return (int) (h * 0.32) + i * (int) (h * 0.09);
    }

    private int rowAt(int y) {
        for (int i = 0; i < labels.size(); i++) {
            if (y >= rowY(i) - (int) (height() * 0.045)
                && y <= rowY(i) + (int) (height() * 0.045)) {
                return i;
            }
        }
        return -1;
    }

    class MenuPanel extends Canvas {
        private Image background;

        public void paint(Graphics g) {
            int w = getWidth() > 0 ? getWidth() : 1920;
            int h = getHeight() > 0 ? getHeight() : 1080;

            if (background == null) {
                background = loadBackground();
            }
            if (background != null) {
                g.drawImage(background, 0, 0, w, h, null);
            } else {
                // Dark vertical gradient, drawn in bands so it stays cheap.
                for (int y = 0; y < h; y += 8) {
                    int shade = 8 + (int) (24.0 * y / h);
                    g.setColor(new Color(shade, shade, shade + 6));
                    g.fillRect(0, y, w, 8);
                }
            }

            int left = (int) (w * 0.08);
            int titleY = (int) (h * 0.16);
            g.setColor(accent);
            g.fillRect(left, titleY + (int) (h * 0.028), (int) (w * 0.05), 3);

            g.setColor(Color.WHITE);
            g.setFont(new Font("SansSerif", Font.BOLD, Math.max(28, h / 15)));
            g.drawString(heading, left, titleY);

            if (discLabel != null && discLabel.length() > 0) {
                g.setColor(new Color(0xB0B0B0));
                g.setFont(new Font("SansSerif", Font.PLAIN, Math.max(14, h / 46)));
                g.drawString(discLabel, left, titleY + (int) (h * 0.055));
            }

            int rowFont = Math.max(20, h / 26);
            int pad = (int) (w * 0.012);
            for (int i = 0; i < labels.size(); i++) {
                int y = rowY(i);
                int rowH = (int) (h * 0.075);
                if (i == selected) {
                    g.setColor(new Color(
                        accent.getRed() / 5, accent.getGreen() / 5,
                        accent.getBlue() / 5));
                    g.fillRect(left - pad, y - rowH + pad, (int) (w * 0.62), rowH);
                    g.setColor(accent);
                    g.fillRect(left - pad, y - rowH + pad, 5, rowH);
                }
                g.setColor(i == selected ? Color.WHITE : new Color(0xC8C8C8));
                g.setFont(new Font("SansSerif", Font.PLAIN, rowFont));
                g.drawString(labels.get(i), left, y);
            }

            g.setColor(new Color(0x8A8A8A));
            g.setFont(new Font("SansSerif", Font.PLAIN, Math.max(12, h / 52)));
            g.drawString("Up / Down to choose     Enter to play",
                left, h - (int) (h * 0.06));
        }
    }
}
