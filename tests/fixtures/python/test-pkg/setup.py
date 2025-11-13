from setuptools import setup, find_packages

setup(
    name='nitro-test-pkg',
    version='1.0.0',
    description='A minimal Python package for integration testing',
    author='Nitro Test',
    author_email='test@nitro.test',
    packages=find_packages(),
    python_requires='>=3.7',
    classifiers=[
        'Development Status :: 3 - Alpha',
        'Intended Audience :: Developers',
        'License :: OSI Approved :: MIT License',
        'Programming Language :: Python :: 3',
        'Programming Language :: Python :: 3.7',
        'Programming Language :: Python :: 3.8',
        'Programming Language :: Python :: 3.9',
        'Programming Language :: Python :: 3.10',
    ],
)
