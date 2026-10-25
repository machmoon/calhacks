import unittest
import numpy as np
from raster import render

class DepthTests(unittest.TestCase):
    def test_overlapping_triangles_are_order_independent(self):
        faces=[[[0,0,0],[10,0,0],[0,10,0]],[[1,1,1],[8,1,1],[1,8,1]]]
        first=render(faces,['blue','red'],240,160)
        second=render(faces[::-1],['red','blue'],240,160)
        np.testing.assert_array_equal(first,second)
        self.assertTrue(((first[:,:,0]>100)&(first[:,:,2]==0)).any())

if __name__=='__main__':unittest.main()
