import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.Map;

public class Words {
    public static void main(String[] args) throws Exception {
        var counts = new HashMap<String, Integer>();
        for (var w : Files.readString(Path.of(args[0])).split("[ \n]+")) {
            if (!w.isEmpty()) counts.merge(w, 1, Integer::sum);
        }
        var all = new ArrayList<>(counts.entrySet());
        all.sort(Map.Entry.<String, Integer>comparingByValue().reversed()
                .thenComparing(Map.Entry.comparingByKey()));
        System.out.println(counts.size());
        for (var e : all.subList(0, 10)) System.out.println(e.getKey() + " " + e.getValue());
    }
}
