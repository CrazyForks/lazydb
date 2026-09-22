import importlib.util
import pathlib
import unittest


SPEC = importlib.util.spec_from_file_location(
    "lazydb_resize", pathlib.Path(__file__).with_name("lazydb_resize.py")
)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class Layout:
    def __init__(self, neighbors, name="splits"):
        self.neighbors = neighbors
        self.name = name

    def neighbors_for_window(self, window, windows):
        return self.neighbors


class Tab:
    def __init__(self, neighbors, name="splits"):
        self.current_layout = Layout(neighbors, name)
        self.windows = []
        self.calls = []

    def resize_window(self, operation, amount):
        self.calls.append((operation, amount))


class Window:
    def __init__(self, tab):
        self.tab = tab


class ResizeTests(unittest.TestCase):
    def test_right_uses_wider_when_right_neighbor_exists(self):
        tab = Tab({"right": [object()]})
        MODULE._resize_direction("right", 3, Window(tab), None)
        self.assertEqual(tab.calls, [("wider", 3)])

    def test_right_uses_narrower_when_only_left_neighbor_exists(self):
        tab = Tab({"left": [object()]})
        MODULE._resize_direction("right", 3, Window(tab), None)
        self.assertEqual(tab.calls, [("narrower", 3)])

    def test_left_uses_narrower_when_right_neighbor_exists(self):
        tab = Tab({"right": [object()]})
        MODULE._resize_direction("left", 3, Window(tab), None)
        self.assertEqual(tab.calls, [("narrower", 3)])

    def test_uses_target_window_tab_not_active_tab(self):
        target = Tab({"right": [object()]})
        active = Tab({"left": [object()]})
        MODULE._resize_direction("right", 3, Window(target), active)
        self.assertEqual(target.calls, [("wider", 3)])
        self.assertEqual(active.calls, [])

    def test_no_axis_neighbor_does_nothing(self):
        tab = Tab({"top": [object()]})
        MODULE._resize_direction("right", 3, Window(tab), None)
        self.assertEqual(tab.calls, [])

    def test_vertical_directions_use_height_operations(self):
        top_only = Tab({"top": [object()]})
        bottom_only = Tab({"bottom": [object()]})
        MODULE._resize_direction("up", 3, Window(top_only), None)
        MODULE._resize_direction("down", 3, Window(bottom_only), None)
        self.assertEqual(top_only.calls, [("taller", 3)])
        self.assertEqual(bottom_only.calls, [("taller", 3)])

    def test_stack_layout_and_invalid_amount_do_nothing(self):
        tab = Tab({"right": [object()]}, name="stack")
        MODULE._resize_direction("right", 3, Window(tab), None)
        self.assertEqual(tab.calls, [])

        boss = type("Boss", (), {"window_id_map": {1: Window(Tab({"right": [object()]}))}})()
        MODULE.handle_result(["lazydb_resize.py", "right", "bad"], None, 1, boss)
        MODULE.handle_result(["lazydb_resize.py", "right", "0"], None, 1, boss)
        self.assertEqual(boss.window_id_map[1].tab.calls, [])


if __name__ == "__main__":
    unittest.main()
