// The window host for `counter.nz` (Gate 3, docs/systems-domains.md Part Q): Cocoa, its state and
// logic the Nazm library's, reached through the C ABI from the event loop's callbacks.
//
//     host           an interactive window: `+`, `-`, `mode`, `reset`
//     host --auto    the same window, driven by a timer's synthetic clicks, printing each label,
//                    and closed after the last — so it checks itself
#import <Cocoa/Cocoa.h>
#include <stdint.h>
#include <string.h>

int64_t nazm_counter_step(int64_t state, int64_t event);
int64_t nazm_counter_count(int64_t state);
int64_t nazm_counter_down(int64_t state);

@interface Counter : NSObject
@property NSTextField *label;
@property NSButton *plus, *minus, *mode, *reset;
@property int64_t state;
@end

@implementation Counter
- (void)show {
    NSString *text = [NSString stringWithFormat:@"%lld %@", (long long)nazm_counter_count(self.state),
                      nazm_counter_down(self.state) ? @"down" : @"up"];
    self.label.stringValue = text;
    printf("%s\n", text.UTF8String);
    fflush(stdout);
}
- (void)event:(NSButton *)sender {
    self.state = nazm_counter_step(self.state, sender.tag);
    [self show];
}
@end

static NSButton *button(NSString *title, NSInteger tag, NSRect frame, Counter *c, NSView *in) {
    NSButton *b = [NSButton buttonWithTitle:title target:c action:@selector(event:)];
    b.tag = tag;
    b.frame = frame;
    [in addSubview:b];
    return b;
}

int main(int argc, const char **argv) {
    @autoreleasepool {
        BOOL autorun = argc > 1 && strcmp(argv[1], "--auto") == 0;
        NSApplication *app = [NSApplication sharedApplication];
        [app setActivationPolicy:NSApplicationActivationPolicyRegular];
        NSWindow *w = [[NSWindow alloc] initWithContentRect:NSMakeRect(200, 200, 320, 120)
                                                  styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable
                                                    backing:NSBackingStoreBuffered
                                                      defer:NO];
        w.title = @"Nazm counter";
        Counter *c = [Counter new];
        c.label = [NSTextField labelWithString:@""];
        c.label.frame = NSMakeRect(20, 70, 280, 30);
        [w.contentView addSubview:c.label];
        c.plus = button(@"+", 1, NSMakeRect(20, 20, 60, 30), c, w.contentView);
        c.minus = button(@"-", 2, NSMakeRect(90, 20, 60, 30), c, w.contentView);
        c.mode = button(@"mode", 3, NSMakeRect(160, 20, 70, 30), c, w.contentView);
        c.reset = button(@"reset", 4, NSMakeRect(240, 20, 70, 30), c, w.contentView);
        [c show];
        [w makeKeyAndOrderFront:nil];
        if (autorun) {
            // The clicks a person would make: three up, a mode switch, two more, a reset.
            NSArray<NSButton *> *script = @[ c.plus, c.plus, c.plus, c.mode, c.plus, c.minus, c.minus, c.reset ];
            __block NSUInteger next = 0;
            [NSTimer scheduledTimerWithTimeInterval:0.05 repeats:YES block:^(NSTimer *t) {
                if (next < script.count) {
                    [script[next++] performClick:nil];
                } else {
                    [t invalidate];
                    [app terminate:nil];
                }
            }];
        }
        [app run];
    }
    return 0;
}
